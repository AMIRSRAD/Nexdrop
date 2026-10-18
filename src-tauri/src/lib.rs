pub mod commands;
pub mod db;
pub mod engine;
pub mod grabber_server;
pub mod process;
pub mod tray;

use std::sync::Arc;
use commands::*;
use engine::DownloadManager;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    if let Err(e) = tauri::Builder::default()
        .setup(|app| {
            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Info)
                        .build(),
                )?;
            }

            let app_data_dir = dirs::data_local_dir()
                .map(|p| p.join("Nexdrop"))
                .unwrap_or_else(|| std::path::PathBuf::from("./nexdrop_data"));

            let _ = std::fs::create_dir_all(&app_data_dir);
            let db_path = app_data_dir.join("nexdrop.db");

            let manager = Arc::new(DownloadManager::new(app.handle().clone(), db_path));
            app.manage(manager.clone());

            // Setup Windows System Tray
            if let Err(err) = tray::setup_tray(app, manager.clone()) {
                log::warn!("Failed to initialize tray icon: {}", err);
            }

            // Start Browser Link Grabber local server on 127.0.0.1:18888
            grabber_server::GrabberServer::start(app.handle().clone(), manager.clone());

            // Startup check: auto resume queued/unfinished downloads if enabled
            manager.apply_runtime_settings();
            manager.recover_interrupted();
            let mgr_clone = manager.clone();
            tauri::async_runtime::spawn(async move {
                let settings = mgr_clone.get_settings();
                if settings.auto_resume_on_launch {
                    mgr_clone.process_queue().await;
                }
            });

            // Start IDM-grade background scheduler loop
            crate::engine::DownloadScheduler::start_loop(app.handle().clone(), manager.clone());

            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                // Keep running in system tray on close
                let _ = window.hide();
                api.prevent_close();
            }
        })
        .invoke_handler(tauri::generate_handler![
            probe_url_cmd,
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
        ])
        .run(tauri::generate_context!()) {
        let msg = format!("error while running tauri application: {e}");
        let _ = std::fs::write("d:\\Projects\\Nexdrop\\tauri_error.log", &msg);
        panic!("{}", msg);
    }
}
