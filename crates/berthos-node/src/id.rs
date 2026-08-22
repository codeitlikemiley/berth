use std::time::{SystemTime, UNIX_EPOCH};

use sha2::{Digest, Sha256};

use crate::error::{Error, Result};

pub(crate) fn hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

pub(crate) fn new_id(prefix: &str) -> String {
    let ns = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{prefix}_{ns:x}")
}

pub(crate) fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

pub(crate) fn sha256_hex(s: &str) -> String {
    hex_lower(&Sha256::digest(s.as_bytes()))
}

pub(crate) fn normalize_code(code: &str) -> String {
    code.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_uppercase())
        .collect()
}

pub(crate) fn random_bytes<const N: usize>() -> Result<[u8; N]> {
    let mut buf = [0u8; N];
    getrandom::fill(&mut buf).map_err(|err| Error::Internal(format!("rng: {err}")))?;
    Ok(buf)
}

pub(crate) fn random_pairing_code() -> Result<String> {
    let bytes = random_bytes::<8>()?;
    const ALPH: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
    let mut s = String::with_capacity(9);
    for (i, b) in bytes.iter().enumerate() {
        if i == 4 {
            s.push('-');
        }
        s.push(ALPH[usize::from(*b) % ALPH.len()] as char);
    }
    Ok(s)
}

pub(crate) fn random_bearer() -> Result<String> {
    let bytes = random_bytes::<32>()?;
    Ok(format!("brt_{}", hex_lower(&bytes)))
}

pub(crate) fn u64_from_i64(v: i64) -> u64 {
    u64::try_from(v).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_strips_dashes_and_case() {
        assert_eq!(normalize_code("ab-cd-ef12"), "ABCDEF12");
        assert_eq!(normalize_code("ABCD-EFGH"), "ABCDEFGH");
    }

    #[test]
    fn pairing_code_shape() {
        let code = random_pairing_code().unwrap();
        assert_eq!(code.len(), 9);
        assert_eq!(code.as_bytes()[4], b'-');
        assert_eq!(normalize_code(&code).len(), 8);
    }
}
