//! While Nexdrop sits hidden in the tray, its web view is asked to give back as much
//! memory as it can (about 380 MB -> 130 MB in total); showing the window restores it.
//!
//! Measured on Windows 11: marking the web view invisible as well (`SetIsVisible(false)`)
//! made WebView2 use noticeably more CPU while hidden, so only the memory level changes.

use tauri::WebviewWindow;

pub fn set_resting(window: &WebviewWindow, resting: bool) {
    #[cfg(windows)]
    {
        let _ = window.with_webview(move |wv| unsafe {
            use webview2_com::Microsoft::Web::WebView2::Win32::{
                ICoreWebView2_19, COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW, COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL,
            };
            use windows_core::Interface;
            if let Ok(core) = wv.controller().CoreWebView2() {
                if let Ok(core19) = core.cast::<ICoreWebView2_19>() {
                    let level = if resting { COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW } else { COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL };
                    let _ = core19.SetMemoryUsageTargetLevel(level);
                }
            }
        });
    }
    #[cfg(not(windows))]
    {
        let _ = (window, resting);
    }
}
