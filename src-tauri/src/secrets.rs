//! Encryption at rest for secrets stored in the database (site passwords,
//! tokens, cookies and auth headers).
//!
//! On Windows this uses DPAPI: the data can only be decrypted by the same
//! Windows user on the same machine, and there is no key for us to manage or
//! leak. Values are stored as `dpapi1:<hex>`. Anything without that prefix is
//! treated as legacy plaintext, so existing databases keep working and get
//! encrypted the next time they are saved.
//!
//! On other platforms values are stored unchanged (the app targets Windows).

const PREFIX: &str = "dpapi1:";

/// Encrypts `plain` for storage. Never fails silently into the *wrong* form:
/// if encryption is unavailable the plaintext is returned (and logged).
pub fn protect(plain: &str) -> String {
    if plain.is_empty() || plain.starts_with(PREFIX) {
        return plain.to_string();
    }
    match platform::protect(plain.as_bytes()) {
        Some(blob) => format!("{PREFIX}{}", hex_encode(&blob)),
        None => {
            log::warn!("Could not encrypt a stored secret; keeping it unencrypted");
            plain.to_string()
        }
    }
}

/// Decrypts a stored value. Returns `None` if it is encrypted but can't be
/// decrypted (for example the database was copied to another user/machine).
pub fn unprotect(stored: &str) -> Option<String> {
    match stored.strip_prefix(PREFIX) {
        None => Some(stored.to_string()),
        Some(hex) => {
            let blob = hex_decode(hex)?;
            let plain = platform::unprotect(&blob)?;
            String::from_utf8(plain).ok()
        }
    }
}

pub fn is_protected(stored: &str) -> bool {
    stored.starts_with(PREFIX)
}

fn hex_encode(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(DIGITS[(b >> 4) as usize] as char);
        out.push(DIGITS[(b & 0xf) as usize] as char);
    }
    out
}

fn hex_decode(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 {
        return None;
    }
    (0..s.len() / 2)
        .map(|i| u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).ok())
        .collect()
}

#[cfg(windows)]
mod platform {
    use std::ptr::{null, null_mut};
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{CryptProtectData, CryptUnprotectData, CRYPT_INTEGER_BLOB};

    fn blob(data: &[u8]) -> CRYPT_INTEGER_BLOB {
        CRYPT_INTEGER_BLOB { cbData: data.len() as u32, pbData: data.as_ptr() as *mut u8 }
    }

    fn take(out: CRYPT_INTEGER_BLOB) -> Vec<u8> {
        let bytes = unsafe { std::slice::from_raw_parts(out.pbData, out.cbData as usize) }.to_vec();
        unsafe { LocalFree(out.pbData as _) };
        bytes
    }

    pub fn protect(data: &[u8]) -> Option<Vec<u8>> {
        let input = blob(data);
        let mut out = CRYPT_INTEGER_BLOB { cbData: 0, pbData: null_mut() };
        let ok = unsafe { CryptProtectData(&input, null(), null(), null(), null(), 0, &mut out) };
        (ok != 0).then(|| take(out))
    }

    pub fn unprotect(data: &[u8]) -> Option<Vec<u8>> {
        let input = blob(data);
        let mut out = CRYPT_INTEGER_BLOB { cbData: 0, pbData: null_mut() };
        let ok = unsafe { CryptUnprotectData(&input, null_mut(), null(), null(), null(), 0, &mut out) };
        (ok != 0).then(|| take(out))
    }
}

#[cfg(not(windows))]
mod platform {
    pub fn protect(_data: &[u8]) -> Option<Vec<u8>> {
        None
    }
    pub fn unprotect(_data: &[u8]) -> Option<Vec<u8>> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_plaintext_passes_through() {
        assert_eq!(unprotect("hunter2").as_deref(), Some("hunter2"));
        assert_eq!(protect(""), "");
    }

    #[test]
    fn hex_round_trip() {
        let data = [0u8, 1, 0x7f, 0x80, 0xff, 0x10];
        assert_eq!(hex_decode(&hex_encode(&data)).unwrap(), data);
        assert!(hex_decode("abc").is_none());
        assert!(hex_decode("zz").is_none());
    }

    #[cfg(windows)]
    #[test]
    fn dpapi_round_trip_hides_the_secret() {
        let secret = "sessionid=abc123; token=päss wörd";
        let stored = protect(secret);
        assert!(is_protected(&stored), "should be encrypted on Windows");
        assert!(!stored.contains("abc123"), "ciphertext must not contain the secret");
        assert_eq!(unprotect(&stored).as_deref(), Some(secret));
        // Protecting twice doesn't double-encrypt.
        assert_eq!(protect(&stored), stored);
        // Corrupted ciphertext is rejected instead of returning garbage.
        assert!(unprotect("dpapi1:00ff00").is_none());
    }
}
