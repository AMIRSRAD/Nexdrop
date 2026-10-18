//! Spawning helpers for external programs.

use std::ffi::OsStr;
use std::process::Command;

/// `CREATE_NO_WINDOW`: stop Windows from flashing a console window when we
/// launch a console program (Defender's MpCmdRun, shutdown, rundll32, ...).
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// A [`Command`] that never opens a visible console window on Windows.
pub fn command<S: AsRef<OsStr>>(program: S) -> Command {
    #[allow(unused_mut)]
    let mut cmd = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}
