//! The `_jmap-peer.<peer-domain>` TXT lookup
//! (draft-atwood-jmap-chat-federation-00 §DNSSEC Key Binding).

use std::future::Future;
use std::time::{Duration, Instant};

use hickory_resolver::Resolver;
use hickory_resolver::config::{ResolverConfig, ResolverOpts};
use hickory_resolver::lookup::Lookup;
use hickory_resolver::net::runtime::TokioRuntimeProvider;
use hickory_resolver::net::{DnsError, NetError};
use hickory_resolver::proto::dnssec::Proof;
use hickory_resolver::proto::rr::{RData, Record, RecordType};
use jmap_peer_auth::DnssecAnswer;

/// The validated result of a TXT query, and how long it may be cached.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DnsAnswer {
    /// The DNSSEC validation result.
    pub answer: DnssecAnswer,
    /// The answer's TTL. §DNSSEC Key Binding: "A receiving server MUST
    /// NOT cache the DNS answer for longer than its TTL." `None` when
    /// there is no answer to cache (a failed lookup) or no TTL is known.
    pub ttl: Option<Duration>,
}

/// A DNSSEC-validated TXT lookup.
///
/// §DNSSEC Key Binding: "A receiving server that uses this binding MUST
/// perform DNSSEC validation itself, or obtain validation results from a
/// resolver over a trusted channel, such as a resolver on the same host
/// or one reached over a channel that authenticates the resolver and
/// protects the integrity of its answers. A receiving server that can do
/// neither MUST treat every zone as unsigned and use the HTTPS-published
/// keys alone." An implementation that cannot meet the first sentence
/// returns [`DnssecAnswer::Insecure`], as [`NoDnssec`] does.
pub trait DnssecLookup: Send + Sync {
    /// Queries the TXT records at `name` (a fully qualified name with a
    /// trailing dot), bypassing any DNS answer this process has cached.
    ///
    /// §Key Publication: on an unknown `kid` the receiving server "also
    /// queries the `_jmap-peer` name again, bypassing any DNS answer it
    /// has cached". The caller caches the answer for at most
    /// [`DnsAnswer::ttl`].
    fn txt(&self, name: &str) -> impl Future<Output = DnsAnswer> + Send;
}

/// A [`DnssecLookup`] for a server without a DNSSEC-validating resolver:
/// every answer is [`DnssecAnswer::Insecure`], so the HTTPS-published
/// keys are used alone.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoDnssec;

impl DnssecLookup for NoDnssec {
    async fn txt(&self, _name: &str) -> DnsAnswer {
        DnsAnswer {
            answer: DnssecAnswer::Insecure,
            ttl: None,
        }
    }
}

/// A [`DnssecLookup`] that validates DNSSEC itself with hickory-resolver.
///
/// Validation runs in this process from the root trust anchor built into
/// hickory, so the upstream name servers need not be trusted for the
/// integrity of their answers (the "perform DNSSEC validation itself"
/// branch of §DNSSEC Key Binding). They must pass DNSSEC records
/// through: an upstream that strips signatures from a signed zone makes
/// the answer bogus, and the request is rejected.
///
/// Each query builds a new resolver, so no answer cache survives between
/// queries and every query bypasses the cache. ponytail: one resolver
/// set-up per key fetch, which happens at most once per cache lifetime or
/// per 60 seconds per peer domain. Upgrade path: a shared resolver with a
/// per-query cache bypass, if hickory grows one.
#[derive(Debug, Clone)]
pub struct HickoryLookup {
    config: ResolverConfig,
    options: ResolverOpts,
}

impl HickoryLookup {
    /// A lookup over the system resolver configuration (`/etc/resolv.conf`
    /// on Unix), with validation turned on.
    ///
    /// # Errors
    ///
    /// The hickory error when the system configuration cannot be read.
    pub fn from_system_conf() -> Result<Self, NetError> {
        let (config, options) = hickory_resolver::system_conf::read_system_conf()?;
        Ok(Self::new(config, options))
    }

    /// A lookup over `config` and `options`, with validation turned on.
    pub fn new(config: ResolverConfig, mut options: ResolverOpts) -> Self {
        options.validate = true;
        Self { config, options }
    }
}

impl DnssecLookup for HickoryLookup {
    async fn txt(&self, name: &str) -> DnsAnswer {
        let resolver =
            Resolver::builder_with_config(self.config.clone(), TokioRuntimeProvider::default())
                .with_options(self.options.clone())
                .build();
        let result = match resolver {
            Ok(resolver) => resolver.lookup(name, RecordType::TXT).await,
            Err(e) => Err(e),
        };
        map_lookup(result, Instant::now())
    }
}

/// Maps a validating hickory lookup to the §DNSSEC Key Binding cases.
///
/// Records of a positive answer each carry their RFC 4033 §5 proof; the
/// answer is as secure as its least secure record. A negative answer is
/// a secure denial when an authority record proving it is secure. Any
/// other failure is [`DnssecAnswer::LookupFailed`].
fn map_lookup(result: Result<Lookup, NetError>, now: Instant) -> DnsAnswer {
    match result {
        Ok(lookup) => {
            let records: Vec<&Record> = lookup
                .answers()
                .iter()
                .filter(|r| r.record_type() != RecordType::RRSIG)
                .collect();
            let has = |proof: Proof| records.iter().any(|r| r.proof == proof);
            let answer = if has(Proof::Bogus) {
                DnssecAnswer::Bogus
            } else if has(Proof::Indeterminate) {
                DnssecAnswer::Indeterminate
            } else if has(Proof::Insecure) {
                DnssecAnswer::Insecure
            } else {
                DnssecAnswer::Secure(records.iter().filter_map(|r| txt_value(r)).collect())
            };
            DnsAnswer {
                answer,
                ttl: Some(lookup.valid_until().saturating_duration_since(now)),
            }
        }
        Err(NetError::Dns(DnsError::DnssecBogus)) => DnsAnswer {
            answer: DnssecAnswer::Bogus,
            ttl: None,
        },
        Err(NetError::Dns(DnsError::Nsec { proof, .. })) => DnsAnswer {
            answer: match proof {
                Proof::Secure => DnssecAnswer::SecureDenial,
                Proof::Insecure => DnssecAnswer::Insecure,
                Proof::Bogus => DnssecAnswer::Bogus,
                Proof::Indeterminate => DnssecAnswer::Indeterminate,
            },
            ttl: None,
        },
        Err(NetError::Dns(DnsError::NoRecordsFound(no_records))) => {
            let authorities = no_records.authorities.as_deref().unwrap_or_default();
            let answer = if authorities.iter().any(|r| r.proof == Proof::Bogus) {
                DnssecAnswer::Bogus
            } else if authorities.iter().any(|r| r.proof == Proof::Secure) {
                DnssecAnswer::SecureDenial
            } else {
                DnssecAnswer::Insecure
            };
            // RFC 2308 §5: the TTL of a negative answer is the lesser of
            // the SOA record's TTL and its MINIMUM field.
            let ttl = no_records
                .soa
                .map(|soa| Duration::from_secs(u64::from(soa.ttl.min(soa.data.minimum))));
            DnsAnswer { answer, ttl }
        }
        Err(_) => DnsAnswer {
            answer: DnssecAnswer::LookupFailed,
            ttl: None,
        },
    }
}

/// A TXT record's value: its character-strings concatenated, if UTF-8.
fn txt_value(record: &Record) -> Option<String> {
    match &record.data {
        RData::TXT(txt) => String::from_utf8(txt.txt_data.concat()).ok(),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use hickory_resolver::net::NoRecords;
    use hickory_resolver::proto::op::{Query, ResponseCode};
    use hickory_resolver::proto::rr::Name;
    use hickory_resolver::proto::rr::rdata::{SOA, TXT};

    use super::*;

    const NAME: &str = "_jmap-peer.alice.example.";
    /// The draft's example `kid` (§Request Signing).
    const RECORD: &str = "v=jmappeer1; kid=kPrK_qmxVWaYVA9wwBF6Iuo3vVzz7TxHCTwXBygrS4k";

    fn name() -> Name {
        Name::from_ascii(NAME).unwrap()
    }

    fn txt(strings: &[&str], proof: Proof) -> Record {
        let data = TXT::new(strings.iter().map(|s| (*s).to_owned()).collect());
        let mut record = Record::from_rdata(name(), 300, RData::TXT(data));
        record.proof = proof;
        record
    }

    fn positive(records: Vec<Record>, now: Instant) -> Result<Lookup, NetError> {
        let query = Query::query(name(), RecordType::TXT);
        Ok(Lookup::new_with_deadline(
            query,
            records,
            now + Duration::from_secs(300),
        ))
    }

    fn negative(authority_proof: Option<Proof>) -> Result<Lookup, NetError> {
        let mut no_records = NoRecords::new(
            Query::query(name(), RecordType::TXT),
            ResponseCode::NXDomain,
        );
        let soa = SOA::new(
            Name::from_ascii("ns.alice.example.").unwrap(),
            Name::from_ascii("hostmaster.alice.example.").unwrap(),
            1,
            3600,
            600,
            86400,
            120,
        );
        let soa_record = Record::from_rdata(Name::from_ascii("alice.example.").unwrap(), 900, soa);
        if let Some(proof) = authority_proof {
            let mut authority = soa_record.clone().into_record_of_rdata();
            authority.proof = proof;
            no_records.authorities = Some(Arc::from(vec![authority]));
        }
        no_records.soa = Some(Box::new(soa_record));
        Err(NetError::Dns(DnsError::NoRecordsFound(no_records)))
    }

    /// RFC 4033 §5 "Secure": an RRset with a chain of trust from a trust
    /// anchor. §DNSSEC Key Binding: "If the answer is secure ... and
    /// contains at least one record of this form" the records bind the
    /// keys, so the TXT values are passed through.
    #[test]
    fn secure_answer_carries_the_txt_values_and_ttl() {
        let now = Instant::now();
        let answer = map_lookup(positive(vec![txt(&[RECORD], Proof::Secure)], now), now);
        assert_eq!(answer.answer, DnssecAnswer::Secure(vec![RECORD.to_owned()]));
        assert_eq!(answer.ttl, Some(Duration::from_secs(300)));
    }

    /// RFC 1035 §3.3.14: TXT-DATA is "One or more <character-string>s";
    /// a value split across strings is matched as their concatenation.
    #[test]
    fn split_character_strings_are_concatenated() {
        let now = Instant::now();
        let (head, tail) = RECORD.split_at(20);
        let answer = map_lookup(positive(vec![txt(&[head, tail], Proof::Secure)], now), now);
        assert_eq!(answer.answer, DnssecAnswer::Secure(vec![RECORD.to_owned()]));
    }

    /// RFC 4033 §5 "Insecure": no chain of trust (an unsigned zone).
    #[test]
    fn insecure_records_are_insecure() {
        let now = Instant::now();
        let records = vec![txt(&[RECORD], Proof::Secure), txt(&["x"], Proof::Insecure)];
        let answer = map_lookup(positive(records, now), now);
        assert_eq!(answer.answer, DnssecAnswer::Insecure);
    }

    /// RFC 4033 §5 "Indeterminate": the resolver cannot tell whether the
    /// RRset should be signed.
    #[test]
    fn indeterminate_records_are_indeterminate() {
        let now = Instant::now();
        let answer = map_lookup(
            positive(vec![txt(&[RECORD], Proof::Indeterminate)], now),
            now,
        );
        assert_eq!(answer.answer, DnssecAnswer::Indeterminate);
    }

    /// RFC 4033 §5 "Bogus": a chain of trust should exist but does not
    /// validate. One bogus record makes the whole answer bogus, so it is
    /// never downgraded to "use the HTTPS-published keys".
    #[test]
    fn a_bogus_record_makes_the_answer_bogus() {
        let now = Instant::now();
        let records = vec![txt(&[RECORD], Proof::Secure), txt(&["x"], Proof::Bogus)];
        let answer = map_lookup(positive(records, now), now);
        assert_eq!(answer.answer, DnssecAnswer::Bogus);
    }

    #[test]
    fn a_bogus_error_is_bogus() {
        let answer = map_lookup(Err(NetError::Dns(DnsError::DnssecBogus)), Instant::now());
        assert_eq!(answer.answer, DnssecAnswer::Bogus);
        assert_eq!(answer.ttl, None);
    }

    /// A negative answer proven by a secure authority record is a
    /// "secure denial of existence"; RFC 2308 §5 gives its TTL as
    /// min(SOA TTL, SOA MINIMUM) = min(900, 120).
    #[test]
    fn secure_negative_answer_is_a_secure_denial() {
        let answer = map_lookup(negative(Some(Proof::Secure)), Instant::now());
        assert_eq!(answer.answer, DnssecAnswer::SecureDenial);
        assert_eq!(answer.ttl, Some(Duration::from_secs(120)));
    }

    #[test]
    fn unproven_negative_answer_is_insecure() {
        assert_eq!(
            map_lookup(negative(None), Instant::now()).answer,
            DnssecAnswer::Insecure
        );
        assert_eq!(
            map_lookup(negative(Some(Proof::Insecure)), Instant::now()).answer,
            DnssecAnswer::Insecure
        );
    }

    #[test]
    fn bogus_negative_answer_is_bogus() {
        assert_eq!(
            map_lookup(negative(Some(Proof::Bogus)), Instant::now()).answer,
            DnssecAnswer::Bogus
        );
    }

    /// §DNSSEC Key Binding: "the lookup fails (for example, SERVFAIL or a
    /// timeout)".
    #[test]
    fn timeout_is_a_failed_lookup() {
        let answer = map_lookup(Err(NetError::Timeout), Instant::now());
        assert_eq!(answer.answer, DnssecAnswer::LookupFailed);
        assert_eq!(answer.ttl, None);
    }
}
