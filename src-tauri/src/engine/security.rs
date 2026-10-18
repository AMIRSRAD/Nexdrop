//! Safety measures applied to files once they have been downloaded.

use std::path::Path;

/// Marks a downloaded file as coming from the Internet (the "Mark of the Web").
///
/// Windows stores this as an NTFS alternate data stream called
/// `Zone.Identifier`. It is what makes SmartScreen check an installer,
/// Office open documents in Protected View, and Explorer show "This file came
/// from another computer". Browsers do this for every download; skipping it
/// would make Nexdrop a way around those protections.
///
/// Best effort: file systems without streams (FAT32/exFAT) simply can't carry
/// the mark, so failures are ignored.
pub fn mark_of_the_web(path: &Path, host_url: Option<&str>, referrer_url: Option<&str>) {
    #[cfg(windows)]
    {
        let body = zone_identifier_contents(host_url, referrer_url);
        let mut stream = path.as_os_str().to_owned();
        stream.push(":Zone.Identifier");
        let _ = std::fs::write(std::path::PathBuf::from(stream), body);
    }
    #[cfg(not(windows))]
    {
        let _ = (path, host_url, referrer_url);
    }
}

/// The text of the `Zone.Identifier` stream. ZoneId 3 = Internet.
pub fn zone_identifier_contents(host_url: Option<&str>, referrer_url: Option<&str>) -> String {
    let mut out = String::from("[ZoneTransfer]\r\nZoneId=3\r\n");
    // Only plain http(s) URLs belong here, and never anything with a line break.
    for (key, value) in [("ReferrerUrl", referrer_url), ("HostUrl", host_url)] {
        if let Some(v) = value.filter(|v| is_safe_url_value(v)) {
            out.push_str(&format!("{key}={v}\r\n"));
        }
    }
    out
}

fn is_safe_url_value(v: &str) -> bool {
    (v.starts_with("http://") || v.starts_with("https://")) && !v.chars().any(|c| c.is_control())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zone_identifier_has_the_expected_shape() {
        let s = zone_identifier_contents(Some("https://example.com/a.zip"), Some("https://example.com/page"));
        assert!(s.starts_with("[ZoneTransfer]\r\nZoneId=3\r\n"));
        assert!(s.contains("ReferrerUrl=https://example.com/page\r\n"));
        assert!(s.contains("HostUrl=https://example.com/a.zip\r\n"));
    }

    #[test]
    fn zone_identifier_refuses_injected_lines_and_odd_schemes() {
        let s = zone_identifier_contents(Some("https://x.test/a\r\nZoneId=0"), Some("file:///C:/secret"));
        assert_eq!(s, "[ZoneTransfer]\r\nZoneId=3\r\n");
    }

    #[cfg(windows)]
    #[test]
    fn writes_the_alternate_data_stream() {
        let dir = std::env::temp_dir().join(format!("nexdrop_motw_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("setup.exe");
        std::fs::write(&file, b"MZ").unwrap();

        mark_of_the_web(&file, Some("https://example.com/setup.exe"), None);

        let mut stream = file.as_os_str().to_owned();
        stream.push(":Zone.Identifier");
        match std::fs::read_to_string(std::path::PathBuf::from(stream)) {
            Ok(text) => assert!(text.contains("ZoneId=3")),
            // Temp may live on a file system without alternate streams.
            Err(_) => eprintln!("skipping: file system has no alternate data streams"),
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
