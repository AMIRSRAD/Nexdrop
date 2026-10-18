//! Positional file I/O for the single-file download target.

use std::fs::{File, OpenOptions};
use std::io;
use std::path::Path;

/// Opens (creating if needed) the download target at exactly `total` bytes.
///
/// Returns `(file, fresh)`; `fresh` is true when the file had to be created or
/// resized, meaning any progress recorded for an earlier file is meaningless.
///
/// On NTFS the file is marked *sparse* before it is extended. Without that,
/// the first write at a high offset makes Windows zero-fill everything before
/// it, stalling the download for as long as it takes to write that much data.
pub fn open_preallocated(path: &Path, total: u64) -> io::Result<(File, bool)> {
    let file = OpenOptions::new().read(true).write(true).create(true).open(path)?;
    if file.metadata()?.len() == total {
        return Ok((file, false));
    }
    file.set_len(0)?;
    mark_sparse(&file);
    file.set_len(total)?;
    Ok((file, true))
}

/// Writes all of `buf` at `offset` without relying on (or disturbing) a shared cursor.
pub fn write_all_at(file: &File, mut buf: &[u8], mut offset: u64) -> io::Result<()> {
    while !buf.is_empty() {
        #[cfg(windows)]
        let n = {
            use std::os::windows::fs::FileExt;
            file.seek_write(buf, offset)?
        };
        #[cfg(unix)]
        let n = {
            use std::os::unix::fs::FileExt;
            file.write_at(buf, offset)?
        };
        if n == 0 {
            return Err(io::Error::new(io::ErrorKind::WriteZero, "failed to write to download file"));
        }
        buf = &buf[n..];
        offset += n as u64;
    }
    Ok(())
}

#[cfg(windows)]
fn mark_sparse(file: &File) {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::System::Ioctl::FSCTL_SET_SPARSE;
    use windows_sys::Win32::System::IO::DeviceIoControl;

    let mut returned: u32 = 0;
    // Best effort: file systems without sparse support (FAT32/exFAT) just refuse.
    unsafe {
        DeviceIoControl(
            file.as_raw_handle() as _,
            FSCTL_SET_SPARSE,
            std::ptr::null(),
            0,
            std::ptr::null_mut(),
            0,
            &mut returned,
            std::ptr::null_mut(),
        );
    }
}

#[cfg(not(windows))]
fn mark_sparse(_file: &File) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preallocates_and_writes_out_of_order() {
        let dir = std::env::temp_dir().join(format!("nexdrop_io_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.bin");

        let (file, fresh) = open_preallocated(&path, 1000).unwrap();
        assert!(fresh);
        write_all_at(&file, b"world", 500).unwrap();
        write_all_at(&file, b"hello", 0).unwrap();
        drop(file);

        // Re-opening a correctly sized file keeps its contents.
        let (file, fresh) = open_preallocated(&path, 1000).unwrap();
        assert!(!fresh);
        drop(file);
        let data = std::fs::read(&path).unwrap();
        assert_eq!(data.len(), 1000);
        assert_eq!(&data[..5], b"hello");
        assert_eq!(&data[500..505], b"world");

        // A different size means a different file: start over.
        let (_file, fresh) = open_preallocated(&path, 2000).unwrap();
        assert!(fresh);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
