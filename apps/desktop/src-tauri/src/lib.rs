//! # JenderalRemote Desktop (Tauri 2 shell)
//!
//! Bridges the React frontend to the Rust engine (rancangan teknis §4.4):
//! the UI only reaches native capabilities through the specific,
//! validated commands below — it can never call input/capture natives
//! without passing the engine's session policy gate.

pub mod commands;
pub mod state;

use state::AppState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    remote_telemetry::init_tracing(false);

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            commands::device_info,
            commands::host_start,
            commands::host_respond,
            commands::host_end,
            commands::controller_connect,
            commands::controller_mouse,
            commands::controller_keyboard,
            commands::controller_end,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
