"""Generate the GroupInfoResponse fixtures of tests/group_info_check.rs.

Independent of the crate under test: the TLS encoding is written here from
draft-ietf-mimi-protocol-06 Section 5.6 and RFC 9420 Sections 2.1.2, 5.1.2
and 5.3, and the signatures come from pyca/cryptography.

Run: python3 -I group_info_response.py
"""

from cryptography.hazmat.primitives import hashes, serialization
from cryptography.hazmat.primitives.asymmetric import ec, ed25519

ROOM = b"mimi://a.example/r/k5qwmzlqmfsgk3tbnvsxi2lf4a"
HUB = b"mimi://a.example"


def opaque(data):
    """opaque <V>: RFC 9420 Section 2.1.2 variable-length prefix."""
    n = len(data)
    if n < 0x40:
        prefix = bytes([n])
    elif n < 0x4000:
        prefix = bytes([0x40 | (n >> 8), n & 0xFF])
    else:
        raise ValueError("too long")
    return prefix + data


def tbs(suite, signature_key):
    """GroupInfoResponseTBS with status success(1)."""
    hub_sender = opaque(signature_key) + b"\x00\x01" + opaque(HUB)
    hpke_ciphertext = opaque(bytes(range(32))) + opaque(b"ciphertext")
    return (
        b"\x01"  # Protocol mls10(1)
        + opaque(ROOM)
        + b"\x01"  # GroupInfoCode success(1)
        + suite.to_bytes(2, "big")
        + hub_sender
        + hpke_ciphertext
    )


def sign_content(payload):
    """RFC 9420 Section 5.1.2 SignContent for label GroupInfoResponseTBS."""
    return opaque(b"MLS 1.0 GroupInfoResponseTBS") + opaque(payload)


# Suite 1 (Ed25519). The private key is the cipher_suite 1 sign_with_label
# "priv" of mlswg/mls-implementations test-vectors/crypto-basics.json.
ed = ed25519.Ed25519PrivateKey.from_private_bytes(
    bytes.fromhex("a2f640dd5005fcad6adb8e9bd8b60d70946bb802e1e788307929fdac81e1ec74")
)
ed_pub = ed.public_key().public_bytes(
    serialization.Encoding.Raw, serialization.PublicFormat.Raw
)
t1 = tbs(1, ed_pub)
print("SUITE1", (t1 + opaque(ed.sign(sign_content(t1)))).hex())

# Suite 2 (ECDSA P-256 SHA-256, DER signature, uncompressed public key).
p = ec.derive_private_key(
    int("207c472d3efaf6737a6f5ae14a3c33a139034865364a128bca5475c85cc02fe0", 16),
    ec.SECP256R1(),
)
p_pub = p.public_key().public_bytes(
    serialization.Encoding.X962, serialization.PublicFormat.UncompressedPoint
)
t2 = tbs(2, p_pub)
print("SUITE2", (t2 + opaque(p.sign(sign_content(t2), ec.ECDSA(hashes.SHA256())))).hex())
