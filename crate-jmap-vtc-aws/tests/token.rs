//! IVS playback tokens verified with RustCrypto `p384`, not ring (the
//! signer).
//!
//! Oracle: "Generate and Sign IVS Playback Tokens",
//! <https://docs.aws.amazon.com/ivs/latest/LowLatencyUserGuide/private-channels-generate-tokens.html>:
//! "To create the signature, use the private key with the algorithm
//! specified in the header (ES384) to sign the encoded header and encoded
//! payload", `ECDSASHA384(base64UrlEncode(header) + "." +
//! base64UrlEncode(payload), <private-key>)`.

use std::time::{Duration, SystemTime};

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use jmap_vtc_aws::ivs::{PlaybackClaims, PlaybackKey};
use p384::ecdsa::signature::Verifier as _;
use p384::ecdsa::{Signature, VerifyingKey};
use ring::rand::SystemRandom;
use ring::signature::{ECDSA_P384_SHA384_FIXED_SIGNING, EcdsaKeyPair};

#[test]
fn es384_signature_verifies_independently() {
    let rng = SystemRandom::new();
    let der = EcdsaKeyPair::generate_pkcs8(&ECDSA_P384_SHA384_FIXED_SIGNING, &rng).unwrap();
    let key = PlaybackKey::from_pkcs8_der(der.as_ref()).unwrap();
    let now = SystemTime::now();
    let token = key
        .sign(
            &PlaybackClaims::new(
                "arn:aws:ivs:us-west-2:123456789012:channel/abcdABCDefgh",
                now + Duration::from_secs(60),
            ),
            now,
        )
        .unwrap();

    let (signing_input, signature) = token.rsplit_once('.').unwrap();
    let verifying = VerifyingKey::from_sec1_bytes(key.public_key_sec1()).unwrap();
    let signature = Signature::from_slice(&URL_SAFE_NO_PAD.decode(signature).unwrap()).unwrap();
    verifying
        .verify(signing_input.as_bytes(), &signature)
        .expect("ES384 over header.payload");

    // A changed payload does not verify.
    let tampered = signing_input.replacen("e", "f", 1);
    assert!(verifying.verify(tampered.as_bytes(), &signature).is_err());
}
