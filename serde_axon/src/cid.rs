//! Content identifiers (CIDs) over Canonical AXON.
//!
//! A CID is `CIDv1(raw, sha2-256)` computed over a value's canonical bytes
//! (§14), rendered as lowercase base32 multibase. Because canonical bytes are
//! deterministic and identical across conforming implementations, so is the
//! CID -- this is what makes "same value -> same CID" hold between `serde_axon`
//! and the reference. Mirrors the reference's `cid2026` / `parse_cid2026` /
//! `verify_cid2026`, and adds [`verify_canonical`] (`verify_canonical2026`).

use alloc::string::String;
use alloc::vec::Vec;

use sha2::{Digest, Sha256};

use crate::error::{Category, Error};
use crate::parser::{Options, base32_decode, base32_encode};
use crate::value::Value;

/// The CIDv1 multihash prefix: version 1, `raw` codec (0x55), sha2-256 (0x12),
/// 32-byte digest length (0x20).
const CID_PREFIX: [u8; 4] = [0x01, 0x55, 0x12, 0x20];

/// Return the `CIDv1(raw, sha2-256)` of `value`'s Canonical AXON bytes, as a
/// lowercase base32 multibase string (a leading `b`, no padding).
pub fn cid(value: &Value) -> Result<String, Error> {
    let canonical = crate::writer::to_string_canonical(value)?;
    let digest = Sha256::digest(canonical.as_bytes());
    let mut binary = Vec::with_capacity(CID_PREFIX.len() + digest.len());
    binary.extend_from_slice(&CID_PREFIX);
    binary.extend_from_slice(&digest);
    let mut out = String::with_capacity(1 + binary.len() * 2);
    out.push('b');
    out.push_str(&base32_encode(&binary));
    Ok(out)
}

/// Validate a CID string and return its 32-byte sha2-256 digest.
///
/// Rejects anything that is not CIDv1 / raw / sha2-256 in canonical lowercase
/// base32 multibase -- matching the reference's `parse_cid2026`.
pub fn parse_cid(text: &str) -> Result<Vec<u8>, Error> {
    let err = |m: &str| Error::new(Category::InvalidLink, m, 0, 0);
    let payload = text
        .strip_prefix('b')
        .ok_or_else(|| err("CID must use lowercase base32 multibase"))?;
    if payload.is_empty()
        || !payload
            .bytes()
            .all(|b| b.is_ascii_lowercase() || (b'2'..=b'7').contains(&b))
    {
        return Err(err("invalid lowercase base32 CID"));
    }
    let binary = base32_decode(payload).map_err(|_| err("invalid base32 CID"))?;
    if binary.len() != 36 || binary[..4] != CID_PREFIX {
        return Err(err("CID must be v1/raw/sha2-256-256"));
    }
    // Reject a non-canonical spelling: re-encoding must reproduce the input.
    if base32_encode(&binary) != payload {
        return Err(err("non-canonical base32 CID"));
    }
    Ok(binary[4..].to_vec())
}

/// True when `cid_text` is the CID of `value`.
pub fn verify_cid(cid_text: &str, value: &Value) -> bool {
    let Ok(digest) = parse_cid(cid_text) else {
        return false;
    };
    let Ok(canonical) = crate::writer::to_string_canonical(value) else {
        return false;
    };
    digest.as_slice() == Sha256::digest(canonical.as_bytes()).as_slice()
}

/// True only when `text` is already in Canonical AXON form.
///
/// Parses under the Graph profile and re-emits canonically; the input is
/// canonical exactly when the render round-trips to itself. Mirrors
/// `verify_canonical2026`.
pub fn verify_canonical(text: &str) -> bool {
    let opts = Options {
        graph: true,
        ..Options::default()
    };
    match crate::de::from_str_stream_with(text, opts) {
        Ok(values) => {
            crate::writer::to_string_stream_canonical(&values).is_ok_and(|value| value == text)
        }
        Err(_) => false,
    }
}
