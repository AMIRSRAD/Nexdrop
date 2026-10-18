//! System tray: the app's home when its window is closed.
//!
//! The menu is a compact control panel: a live status line, the things you
//! reach for most (open, add, pause/resume everything, the downloads folder),
//! the start-with-Windows switch, and Exit. Closing the window only hides it;
//! Exit here is the one way to actually quit.

use std::sync::Arc;
use std::time::Duration;

use tauri::{
    image::Image,
    menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager,
};
use tauri_plugin_autostart::ManagerExt;

use crate::engine::DownloadManager;
use crate::util::format_speed;

const TRAY_ID: &str = "main";

/// Brings the main window to the front (creating no new window).
pub fn show_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

fn toggle_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        if window.is_visible().unwrap_or(false) && !window.is_minimized().unwrap_or(false) {
            let _ = window.hide();
        } else {
            show_main(app);
        }
    }
}

struct Items {
    status: MenuItem<tauri::Wry>,
    pause_all: MenuItem<tauri::Wry>,
    resume_all: MenuItem<tauri::Wry>,
    autostart: CheckMenuItem<tauri::Wry>,
}

pub fn setup_tray(app: &tauri::App, manager: Arc<DownloadManager>) -> Result<(), Box<dyn std::error::Error>> {
    let handle = app.handle();

    let status = MenuItem::with_id(handle, "status", "Nexdrop", false, None::<&str>)?;
    let open = MenuItem::with_id(handle, "show", "Open Nexdrop", true, None::<&str>)?;
    let add = MenuItem::with_id(handle, "add", "Add download…", true, None::<&str>)?;
    let add_torrent = MenuItem::with_id(handle, "add_torrent", "Add torrent…", true, None::<&str>)?;
    let pause_all = MenuItem::with_id(handle, "pause_all", "Pause all", false, None::<&str>)?;
    let resume_all = MenuItem::with_id(handle, "resume_all", "Resume all", false, None::<&str>)?;
    let folder = MenuItem::with_id(handle, "folder", "Open downloads folder", true, None::<&str>)?;
    let settings = MenuItem::with_id(handle, "settings", "Settings", true, None::<&str>)?;
    let autostart_on = handle.autolaunch().is_enabled().unwrap_or(false);
    let autostart = CheckMenuItem::with_id(handle, "autostart", "Start with Windows", true, autostart_on, None::<&str>)?;
    let quit = MenuItem::with_id(handle, "quit", "Exit Nexdrop", true, None::<&str>)?;

    let sep = || PredefinedMenuItem::separator(handle);
    let menu = Menu::with_items(
        handle,
        &[
            &status,
            &sep()?,
            &open,
            &add,
            &add_torrent,
            &sep()?,
            &pause_all,
            &resume_all,
            &folder,
            &sep()?,
            &settings,
            &autostart,
            &sep()?,
            &quit,
        ],
    )?;

    let icon: Image<'_> = app
        .default_window_icon()
        .cloned()
        .ok_or("The app icon is missing, so the tray icon can't be created")?;

    let menu_manager = manager.clone();
    TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon)
        .tooltip("Nexdrop")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(move |app: &AppHandle, event| match event.id().as_ref() {
            "show" => show_main(app),
            "add" => {
                show_main(app);
                let _ = app.emit("open-new-download", ());
            }
            "add_torrent" => {
                show_main(app);
                let _ = app.emit("torrent-requested", serde_json::json!({ "source": "" }));
            }
            "settings" => {
                show_main(app);
                let _ = app.emit("open-settings", ());
            }
            "pause_all" => menu_manager.pause_all(),
            "resume_all" => {
                let mgr = menu_manager.clone();
                tauri::async_runtime::spawn(async move { mgr.resume_all().await });
            }
            "folder" => {
                let dir = menu_manager.get_settings().default_destination;
                let _ = std::fs::create_dir_all(&dir);
                let _ = open::that(dir);
            }
            "autostart" => {
                let launcher = app.autolaunch();
                let enabled = launcher.is_enabled().unwrap_or(false);
                let _ = if enabled { launcher.disable() } else { launcher.enable() };
            }
            "quit" => crate::quit_app(app),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                toggle_main(tray.app_handle());
            }
        })
        .build(handle)?;

    let items = Items { status, pause_all, resume_all, autostart };
    spawn_status_updater(handle.clone(), manager, items);
    Ok(())
}

/// Keeps the status line, tooltip and enabled states current.
fn spawn_status_updater(app: AppHandle, manager: Arc<DownloadManager>, items: Items) {
    tauri::async_runtime::spawn(async move {
        let mut last = String::new();
        loop {
            tokio::time::sleep(Duration::from_secs(2)).await;
            let summary = manager.status_summary();
            let line = status_line(summary.downloading, summary.queued, summary.paused, summary.speed_bps);
            let key = format!("{line}|{}", summary.paused + summary.queued + summary.downloading);
            if key != last {
                last = key;
                let _ = items.status.set_text(&line);
                let _ = items.pause_all.set_enabled(summary.downloading + summary.queued > 0);
                let _ = items.resume_all.set_enabled(summary.paused > 0);
                if let Some(tray) = app.tray_by_id(TRAY_ID) {
                    let _ = tray.set_tooltip(Some(if summary.downloading > 0 { format!("Nexdrop — {line}") } else { "Nexdrop".to_string() }));
                }
            }
            // Keep the check mark honest if the setting was changed elsewhere (e.g. in Settings).
            let _ = items.autostart.set_checked(app.autolaunch().is_enabled().unwrap_or(false));
        }
    });
}

pub fn status_line(downloading: u32, queued: u32, paused: u32, speed_bps: u64) -> String {
    if downloading > 0 {
        let more = if queued > 0 { format!(" · {queued} queued") } else { String::new() };
        format!("{downloading} downloading · {}{more}", format_speed(speed_bps))
    } else if queued > 0 {
        format!("{queued} queued")
    } else if paused > 0 {
        format!("{paused} paused")
    } else {
        "Idle".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_line_describes_the_queue() {
        assert_eq!(status_line(0, 0, 0, 0), "Idle");
        assert_eq!(status_line(0, 0, 3, 0), "3 paused");
        assert_eq!(status_line(0, 2, 1, 0), "2 queued");
        assert_eq!(status_line(2, 1, 0, 3 * 1024 * 1024), "2 downloading · 3.0 MB/s · 1 queued");
    }
}
