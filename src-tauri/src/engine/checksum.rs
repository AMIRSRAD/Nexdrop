use std::fs::File;
use std::io::{Read, Result as IoResult};
use std::path::Path;
use md5::Md5;
use sha2::{Digest, Sha256};
use crate::db::models::ChecksumResult;

pub fn compute_checksums<P: AsRef<Path>>(path: P) -> IoResult<(String, String)> {
    let mut file = File::open(path)?;
    let mut sha_hasher = Sha256::new();
    let mut md5_hasher = Md5::new();

    let mut buffer = [0u8; 65536]; // 64KB read buffer
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        sha_hasher.update(&buffer[..count]);
        md5_hasher.update(&buffer[..count]);
    }

    let sha256_result = format!("{:x}", sha_hasher.finalize());
    let md5_result = format!("{:x}", md5_hasher.finalize());

    Ok((sha256_result, md5_result))
}

pub fn verify_file_checksum<P: AsRef<Path>>(
    path: P,
    algorithm: &str,
    expected: Option<&str>,
) -> Result<ChecksumResult, String> {
    let (sha256, md5) = compute_checksums(path).map_err(|e| format!("Failed to read file: {e}"))?;

    let algo_lower = algorithm.to_lowercase();
    let calculated = match algo_lower.as_str() {
        "md5" => md5,
        _ => sha256,
    };

    let matches = if let Some(exp) = expected {
        calculated.eq_ignore_ascii_case(exp.trim())
    } else {
        true
    };

    Ok(ChecksumResult {
        algorithm: if algo_lower == "md5" { "MD5".to_string() } else { "SHA-256".to_string() },
        calculated_hash: calculated,
        expected_hash: expected.map(|s| s.trim().to_string()),
        matches,
    })
}

/// What a user-supplied expected hash turned out to be.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HashKind {
    Md5,
    Sha256,
}

/// Cleans up a pasted hash (whitespace, `sha256:` / `md5:` prefix, case) and
/// works out which algorithm it is from its length. `Ok(None)` for an empty
/// value; `Err` for something that can't be an MD5 or SHA-256 hash.
pub fn normalize_expected_checksum(raw: &str) -> Result<Option<(HashKind, String)>, String> {
    let mut s = raw.trim().to_ascii_lowercase();
    if s.is_empty() {
        return Ok(None);
    }
    for prefix in ["sha256:", "sha-256:", "md5:"] {
        if let Some(rest) = s.strip_prefix(prefix) {
            s = rest.trim().to_string();
            break;
        }
    }
    // `hash  filename` / `hash *filename` (the .sha256 file format)
    if let Some(first) = s.split_whitespace().next() {
        s = first.to_string();
    }
    if !s.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err("The expected checksum must be hexadecimal (0-9, a-f)".to_string());
    }
    match s.len() {
        32 => Ok(Some((HashKind::Md5, s))),
        64 => Ok(Some((HashKind::Sha256, s))),
        n => Err(format!(
            "The expected checksum has {n} characters; an MD5 has 32 and a SHA-256 has 64"
        )),
    }
}

/// Compares the file's real hashes to the one the user expected.
/// Returns `Ok(())` on a match, or a message describing the mismatch.
pub fn check_expected(expected: &str, sha256: &str, md5: &str) -> Result<(), String> {
    match normalize_expected_checksum(expected) {
        Ok(Some((HashKind::Sha256, want))) if want != sha256.to_ascii_lowercase() => {
            Err(format!("SHA-256 mismatch: expected {want}, got {sha256}"))
        }
        Ok(Some((HashKind::Md5, want))) if want != md5.to_ascii_lowercase() => {
            Err(format!("MD5 mismatch: expected {want}, got {md5}"))
        }
        Ok(_) => Ok(()),
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHA: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
    const MD5: &str = "d41d8cd98f00b204e9800998ecf8427e";

    #[test]
    fn recognises_hash_kinds_and_cleans_input() {
        assert_eq!(normalize_expected_checksum(SHA).unwrap(), Some((HashKind::Sha256, SHA.to_string())));
        assert_eq!(normalize_expected_checksum(&format!("  SHA256:{}  ", SHA.to_uppercase())).unwrap().unwrap().1, SHA);
        assert_eq!(normalize_expected_checksum(&format!("{MD5} *file.iso")).unwrap(), Some((HashKind::Md5, MD5.to_string())));
        assert_eq!(normalize_expected_checksum("   ").unwrap(), None);
    }

    #[test]
    fn rejects_things_that_are_not_hashes() {
        assert!(normalize_expected_checksum("not a hash").is_err());
        assert!(normalize_expected_checksum("abc123").unwrap_err().contains("6 characters"));
        assert!(normalize_expected_checksum(&"g".repeat(64)).is_err());
    }

    #[test]
    fn verification_detects_mismatches() {
        assert!(check_expected(SHA, SHA, MD5).is_ok());
        assert!(check_expected(MD5, SHA, MD5).is_ok());
        assert!(check_expected(&SHA.to_uppercase(), SHA, MD5).is_ok());
        let flipped = format!("{}0", &SHA[..63]);
        assert!(check_expected(&flipped, SHA, MD5).unwrap_err().contains("mismatch"));
        let wrong_md5 = "0".repeat(32);
        assert!(check_expected(&wrong_md5, SHA, MD5).unwrap_err().contains("MD5 mismatch"));
        assert!(check_expected("", SHA, MD5).is_ok());
    }
}
