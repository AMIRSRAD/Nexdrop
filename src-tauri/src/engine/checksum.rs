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
