use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use commands::*;
use tauri::Window;

pub type ShareableWindow = Arc<Mutex<Window>>;

pub struct RunnerInstance {
    pub cancel: Arc<AtomicBool>,
    pub terminator: tokio::sync::oneshot::Sender<()>,
}

pub struct AppState {
    pub runner_instance: Arc<Mutex<Option<RunnerInstance>>>,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            runner_instance: Arc::new(Mutex::new(None)),
        }
    }
}

mod commands;

pub fn gui_main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_window_state::Builder::default().build())
        .manage(AppState::new())
        .invoke_handler(tauri::generate_handler![
            sys_memory,
            get_options,
            store_options,
            run_client,
            login_offline,
            login_microsoft,
            refresh,
            logout,
            default_data_folder_path,
            terminate,
            get_launcher_version,
            fetch_latest_release,
            get_predefined_mods,
            set_predefined_mod_enabled,
            get_custom_mods,
            install_custom_mod,
            toggle_custom_mod,
            delete_custom_mod
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
