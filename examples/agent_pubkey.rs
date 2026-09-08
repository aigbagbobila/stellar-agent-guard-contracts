//! Derive the raw Ed25519 public key (hex, 32 bytes) from a Stellar secret
//! key. The guard contract registers the agent as `BytesN<32>` — the raw
//! Ed25519 public key — not the `G...` address, so this is the bridge between
//! a standard `stellar keys` identity and `PolicyEngine::initialize`.
//!
//! Usage:
//! ```text
//! echo '<SECRET>' | cargo run --example agent_pubkey
//! ```
//!
//! A Stellar secret key is a strkey: version byte `0x90` + 32-byte Ed25519
//! seed + 2-byte CRC16-XModem checksum, base32-encoded (RFC 4648, no
//! padding). The seed *is* the Ed25519 signing seed, so the derived public
//! key matches the `G...` address the identity resolves to.

use ed25519_dalek::SigningKey;
use std::io::Read;

const ALPHABET: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";

fn base32_decode(input: &str) -> Option<Vec<u8>> {
    let mut acc: u32 = 0;
    let mut bits: u32 = 0;
    let mut out = Vec::new();
    for c in input.bytes() {
        if c == b'=' {
            break;
        }
        let v = u32::try_from(ALPHABET.iter().position(|&a| a == c)?).ok()?;
        acc = (acc << 5) | v;
        bits += 5;
        while bits >= 8 {
            bits -= 8;
            out.push(u8::try_from((acc >> bits) & 0xff).ok()?);
        }
        // Drop the bits already emitted as bytes so the stale high bits can
        // never re-enter the next byte on the following shift.
        acc &= (1u32 << bits) - 1;
    }
    Some(out)
}

fn main() {
    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input).expect("stdin");
    let secret = input.trim();
    let raw = base32_decode(secret).expect("invalid base32 strkey");
    assert_eq!(raw.len(), 35, "strkey must decode to 35 bytes");
    assert_eq!(raw[0], 0x90, "not a strkey secret key (version byte)");
    let seed: [u8; 32] = raw[1..33].try_into().expect("seed length");
    let signing = SigningKey::from_bytes(&seed);
    for b in signing.verifying_key().to_bytes() {
        print!("{b:02x}");
    }
    println!();
}
