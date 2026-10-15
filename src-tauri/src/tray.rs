use std::sync::Arc;
use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager,
};
use crate::engine::DownloadManager;

pub fn setup_tray(app: &tauri::App, manager: Arc<DownloadManager>) -> Result<(), Box<dyn std::error::Error>> {
    let handle = app.handle();

    let show_item = MenuItem::with_id(handle, "show", "Open Nexdrop", true, None::<&str>)?;
    let add_item = MenuItem::with_id(handle, "add", "Add Download...", true, None::<&str>)?;
    let sep1 = PredefinedMenuItem::separator(handle)?;
    let pause_all_item = MenuItem::with_id(handle, "pause_all", "Pause All Downloads", true, None::<&str>)?;
    let resume_all_item = MenuItem::with_id(handle, "resume_all", "Resume All Downloads", true, None::<&str>)?;
    let sep2 = PredefinedMenuItem::separator(handle)?;
    let quit_item = MenuItem::with_id(handle, "quit", "Exit Nexdrop", true, None::<&str>)?;

    let menu = Menu::with_items(
        handle,
        &[
            &show_item,
            &add_item,
            &sep1,
            &pause_all_item,
            &resume_all_item,
            &sep2,
            &quit_item,
        ],
    )?;

    let icon = app
        .default_window_icon()
        .cloned()
        .ok_or("Failed to load default window icon for tray")?;

    let mgr_for_menu = manager.clone();

    let _tray = TrayIconBuilder::new()
        .icon(icon)
        .tooltip("Nexdrop — Internet Download Manager")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(move |app: &AppHandle, event| {
            let id = event.id().as_ref();
            match id {
                "show" => {
                    if let Some(window) = app.get_webview_window("main") {
                        let _ = window.show();
                        let _ = window.unminimize();
                        let _ = window.set_focus();
                    }
                }
                "add" => {
                    if let Some(window) = app.get_webview_window("main") {
                        let _ = window.show();
                        let _ = window.unminimize();
                        let _ = window.set_focus();
                        let _ = window.emit("open-new-download", ());
                    }
                }
                "pause_all" => {
                    let mgr = mgr_for_menu.clone();
                    if let Ok(tasks) = mgr.get_all_downloads() {
                        for task in tasks {
                            if task.status == crate::db::models::DownloadStatus::Downloading {
                                let _ = mgr.pause_download(&task.id);
                            }
                        }
                    }
                }
                "resume_all" => {
                    let mgr = mgr_for_menu.clone();
                    tauri::async_runtime::spawn(async move {
                        if let Ok(tasks) = mgr.get_all_downloads() {
                            for task in tasks {
                                if task.status == crate::db::models::DownloadStatus::Paused
                                    || task.status == crate::db::models::DownloadStatus::Queued
                                {
                                    let _ = mgr.resume_download(&task.id).await;
                                }
                            }
                        }
                    });
                }
                "quit" => {
                    app.exit(0);
                }
                _ => {}
            }
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                let app = tray.app_handle();
                if let Some(window) = app.get_webview_window("main") {
                    if window.is_visible().unwrap_or(false) {
                        let _ = window.hide();
                    } else {
                        let _ = window.show();
                        let _ = window.unminimize();
                        let _ = window.set_focus();
                    }
                }
            }
        })
        .build(handle)?;

    Ok(())
}
