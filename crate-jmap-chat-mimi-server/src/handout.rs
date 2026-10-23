//! draft-atwood-jmap-chat-mimi-00 §Handing Out KeyPackages: which stored
//! KeyPackage a key material request may receive.

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use jmap_chat_mimi_types::Base64Url;
use openmls::prelude::{
    KeyPackageIn, OpenMlsCrypto, ProtocolVersion, RequiredCapabilitiesExtension,
};
use tls_codec::Deserialize as _;

/// What a key material request accepts (draft-ietf-mimi-protocol-06 §5.2):
/// "a non-empty list of acceptable MLS ciphersuites" and an MLS
/// `RequiredCapabilities` object.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyMaterialRequirements {
    /// The request's `acceptableCiphersuites`.
    pub acceptable_ciphersuites: Vec<u16>,
    /// The request's `requiredCapabilities`.
    pub required_capabilities: RequiredCapabilitiesExtension,
}

/// How a stored KeyPackage fits a [`KeyMaterialRequirements`].
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyPackageFit {
    /// The KeyPackage may be handed out for the request.
    Compatible,
    /// The KeyPackage is valid now but does not meet the request: the
    /// client's `clientStatus` is `nothingCompatible` when none of its
    /// KeyPackages is compatible.
    Incompatible,
    /// The KeyPackage cannot be handed out at all, for example because
    /// its leaf node lifetime has passed: "The target provider MUST NOT
    /// provide expired keying material" (draft-ietf-mimi-protocol-06 §5.2).
    Unavailable,
}

impl KeyMaterialRequirements {
    /// Requirements from a request's `acceptableCiphersuites` and
    /// `requiredCapabilities`.
    pub fn new(
        acceptable_ciphersuites: Vec<u16>,
        required_capabilities: RequiredCapabilitiesExtension,
    ) -> Self {
        Self {
            acceptable_ciphersuites,
            required_capabilities,
        }
    }

    /// How the stored `key_package` fits these requirements.
    ///
    /// The KeyPackage is validated again (RFC 9420 §10.1) so that one
    /// whose lifetime has passed since it was stored is
    /// [`KeyPackageFit::Unavailable`]. It is
    /// [`KeyPackageFit::Compatible`] when its cipher suite is acceptable
    /// and its leaf node capabilities list every required credential type
    /// and every required extension and proposal type that is not
    /// "default" (RFC 9420 §7.2: proposal types 0x0001 to 0x0007 and
    /// extension types 0x0001 to 0x0005, which §11.1 says "are assumed to
    /// be implemented by all clients"; "Note that this is not true for
    /// credential types").
    pub fn fit<C: OpenMlsCrypto>(&self, crypto: &C, key_package: &Base64Url) -> KeyPackageFit {
        let Some(key_package) = URL_SAFE_NO_PAD
            .decode(key_package.as_str())
            .ok()
            .and_then(|bytes| KeyPackageIn::tls_deserialize_exact(bytes).ok())
            .and_then(|kp| kp.validate(crypto, ProtocolVersion::Mls10).ok())
        else {
            return KeyPackageFit::Unavailable;
        };
        let capabilities = key_package.leaf_node().capabilities();
        let required = &self.required_capabilities;
        let compatible =
            self.acceptable_ciphersuites
                .contains(&u16::from(key_package.ciphersuite()))
                && required.extension_types().iter().all(|e| {
                    matches!(u16::from(*e), 1..=5) || capabilities.extensions().contains(e)
                })
                && required.proposal_types().iter().all(|p| {
                    matches!(u16::from(*p), 1..=7) || capabilities.proposals().contains(p)
                })
                && required
                    .credential_types()
                    .iter()
                    .all(|c| capabilities.credentials().contains(c));
        if compatible {
            KeyPackageFit::Compatible
        } else {
            KeyPackageFit::Incompatible
        }
    }
}
