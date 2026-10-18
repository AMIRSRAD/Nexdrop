//! Small shared helpers.

/// "3.0 MB/s"-style speed for status text.
pub fn format_speed(bytes_per_sec: u64) -> String {
    const UNITS: [&str; 4] = ["B/s", "KB/s", "MB/s", "GB/s"];
    let mut value = bytes_per_sec as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes_per_sec} B/s")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

/// Where Nexdrop keeps its database and small state files.
pub fn data_dir() -> std::path::PathBuf {
    dirs::data_local_dir()
        .map(|p| p.join("Nexdrop"))
        .unwrap_or_else(|| std::path::PathBuf::from("./nexdrop_data"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_speeds() {
        assert_eq!(format_speed(0), "0 B/s");
        assert_eq!(format_speed(900), "900 B/s");
        assert_eq!(format_speed(1536), "1.5 KB/s");
        assert_eq!(format_speed(5 * 1024 * 1024), "5.0 MB/s");
    }
}
