//! Desktop notifications and sounds.
//!
//! Windows toasts are shown directly through WinRT so that the user's
//! "play a sound" preference can silence them (the generic notification plugin
//! offers no way to do that).

use tauri::{AppHandle, Manager};

/// True if the main window is in front of the user (so a toast would be noise).
pub fn window_has_focus(app: &AppHandle) -> bool {
    app.get_webview_window("main")
        .map(|w| w.is_focused().unwrap_or(false) && w.is_visible().unwrap_or(false))
        .unwrap_or(false)
}

/// Shows a desktop notification. Fire-and-forget; failures are only logged.
pub fn toast(app: &AppHandle, title: &str, body: &str, sound: bool) {
    #[cfg(windows)]
    {
        let (title, body) = (title.to_string(), body.to_string());
        let app_id = app_user_model_id(app);
        // Showing a toast talks to COM; keep it off the async threads.
        tauri::async_runtime::spawn_blocking(move || {
            use tauri_winrt_notification::{Sound, Toast};
            let result = Toast::new(&app_id)
                .title(&title)
                .text1(&body)
                .sound(if sound { Some(Sound::Default) } else { None })
                .show();
            if let Err(e) = result {
                log::warn!("Could not show notification: {e}");
            }
        });
    }
    #[cfg(not(windows))]
    {
        let _ = (app, title, body, sound);
    }
}

/// Plays the system "asterisk" sound (used when no toast is shown).
pub fn beep() {
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::Diagnostics::Debug::MessageBeep;
        use windows_sys::Win32::UI::WindowsAndMessaging::MB_ICONASTERISK;
        unsafe {
            MessageBeep(MB_ICONASTERISK);
        }
    }
}

/// Toasts need an AppUserModelID. An installed app has its own; a build run
/// straight out of `target/` is not registered, so it borrows PowerShell's.
#[cfg(windows)]
fn app_user_model_id(app: &AppHandle) -> String {
    let sep = std::path::MAIN_SEPARATOR;
    let in_target_dir = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|p| p.display().to_string()))
        .map(|dir| dir.ends_with(&format!("{sep}target{sep}debug")) || dir.ends_with(&format!("{sep}target{sep}release")))
        .unwrap_or(false);
    if in_target_dir {
        tauri_winrt_notification::Toast::POWERSHELL_APP_ID.to_string()
    } else {
        app.config().identifier.clone()
    }
}
