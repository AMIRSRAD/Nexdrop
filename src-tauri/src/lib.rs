pub mod commands;
pub mod db;
pub mod engine;
pub mod grabber_server;
pub mod notify;
pub mod power;
pub mod process;
pub mod secrets;
pub mod tray;
pub mod util;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use commands::*;
use engine::DownloadManager;
use tauri::{AppHandle, Manager, RunEvent};
use tauri_plugin_autostart::MacosLauncher;

/// Passed by the "start with Windows" entry so the app starts hidden in the tray.
const MINIMIZED_FLAG: &str = "--minimized";

static QUITTING: AtomicBool = AtomicBool::new(false);

/// Quits for real (the window's close button only hides to the tray).
///
/// Running downloads are stopped cleanly first: their progress is saved and
/// they stay marked as in progress, so they continue on the next launch.
pub fn quit_app(app: &AppHandle) {
    if QUITTING.swap(true, Ordering::SeqCst) {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Some(manager) = app.try_state::<Arc<DownloadManager>>() {
            manager.shutdown().await;
        }
        app.exit(0);
    });
}

/// First time the window is hidden, say where the app went (once, ever).
fn tell_about_the_tray_once(app: &AppHandle) {
    let flag = util::data_dir().join("tray_hint_shown");
    if flag.exists() {
        return;
    }
    let _ = std::fs::create_dir_all(util::data_dir());
    let _ = std::fs::write(&flag, "1");
    notify::toast(
        app,
        "Nexdrop is still running",
        "Downloads continue in the background. Use the tray icon to open it again, or choose Exit to quit.",
        false,
    );
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let result = tauri::Builder::default()
        // Must be first: a second launch hands over to the running instance
        // (which brings its window forward) instead of fighting it for the
        // database and the browser-bridge port.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            tray::show_main(app);
        }))
        .plugin(tauri_plugin_autostart::init(MacosLauncher::LaunchAgent, Some(vec![MINIMIZED_FLAG])))
        .setup(|app| {
            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Info)
                        .build(),
                )?;
            }

            let app_data_dir = util::data_dir();
            let _ = std::fs::create_dir_all(&app_data_dir);
            let db_path = app_data_dir.join("nexdrop.db");

            let manager = Arc::new(DownloadManager::new(app.handle().clone(), db_path));
            app.manage(manager.clone());

            // System tray
            if let Err(err) = tray::setup_tray(app, manager.clone()) {
                log::warn!("Failed to initialize tray icon: {}", err);
            }

            // Browser bridge on 127.0.0.1:18888
            grabber_server::GrabberServer::start(app.handle().clone(), manager.clone());

            // Resume what was running (or wait for the scheduler window)
            manager.protect_stored_secrets();
            let mgr_clone = manager.clone();
            tauri::async_runtime::spawn(async move {
                mgr_clone.startup_resume().await;
            });

            // Background scheduler loop
            crate::engine::DownloadScheduler::start_loop(app.handle().clone(), manager.clone());

            // The window starts hidden (no white flash while the page loads).
            // Show it, unless we were started with Windows and belong in the tray.
            if !std::env::args().any(|a| a == MINIMIZED_FLAG) {
                tray::show_main(app.handle());
            }

            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                // Closing the window keeps Nexdrop running in the tray.
                let _ = window.hide();
                api.prevent_close();
                tell_about_the_tray_once(window.app_handle());
            }
        })
        .invoke_handler(tauri::generate_handler![
            probe_url_cmd,
            probe_stream_cmd,
            get_queues_cmd,
            save_queue_cmd,
            delete_queue_cmd,
            move_to_queue_cmd,
            start_queue_cmd,
            stop_queue_cmd,
            add_download_cmd,
            batch_add_downloads_cmd,
            pause_download_cmd,
            resume_download_cmd,
            retry_download_cmd,
            cancel_download_cmd,
            delete_download_cmd,
            get_all_downloads_cmd,
            get_download_cmd,
            get_settings_cmd,
            update_settings_cmd,
            open_file_cmd,
            reveal_file_cmd,
            get_default_download_dir_cmd,
            verify_checksum_cmd,
            calculate_file_hash_cmd,
            get_curl_command_cmd,
            export_downloads_cmd,
            get_site_credentials_cmd,
            save_site_credential_cmd,
            delete_site_credential_cmd,
            export_checksum_file_cmd,
            parse_curl_command_cmd,
            grab_page_links_cmd,
            cancel_completion_action_cmd,
            probe_torrent_cmd,
            add_torrent_cmd,
            stop_seeding_cmd,
            start_seeding_cmd,
            quit_app_cmd,
            get_autostart_cmd,
            set_autostart_cmd,
            get_extension_dir_cmd,
            open_extension_dir_cmd,
            get_app_info_cmd,
        ])
        .build(tauri::generate_context!());

    let app = match result {
        Ok(app) => app,
        Err(e) => {
            // No console in release builds: leave a log the user can send us.
            let msg = format!("Nexdrop could not start: {e}\n");
            eprintln!("{msg}");
            let _ = std::fs::create_dir_all(util::data_dir());
            let _ = std::fs::write(util::data_dir().join("startup_error.log"), &msg);
            std::process::exit(1);
        }
    };

    app.run(|_app, event| {
        if let RunEvent::ExitRequested { api, code, .. } = event {
            // `code` is None when the last window closed; we live in the tray, so stay.
            // An explicit exit (Exit in the tray, Ctrl+Q) carries a code and goes through.
            if code.is_none() {
                api.prevent_exit();
            }
        }
    });
}
