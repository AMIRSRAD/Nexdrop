//! Putting the computer to sleep or shutting it down (for "when finished" and
//! scheduler actions). Callers are responsible for giving the user a chance to
//! cancel first (see `DownloadManager::schedule_power_action`).

pub fn sleep_computer() {
    #[cfg(target_os = "windows")]
    {
        let _ = crate::process::command("rundll32.exe")
            .args(["powrprof.dll,SetSuspendState", "0,1,0"])
            .spawn();
    }
}

pub fn shutdown_computer() {
    #[cfg(target_os = "windows")]
    {
        // Immediate: the grace period (and the chance to cancel) has already happened in the app.
        let _ = crate::process::command("shutdown").args(["/s", "/t", "0"]).spawn();
    }
}

pub fn hibernate_computer() {
    #[cfg(target_os = "windows")]
    {
        let _ = crate::process::command("shutdown").args(["/h"]).spawn();
    }
}

pub fn lock_computer() {
    #[cfg(target_os = "windows")]
    {
        let _ = crate::process::command("rundll32.exe").args(["user32.dll,LockWorkStation"]).spawn();
    }
}
