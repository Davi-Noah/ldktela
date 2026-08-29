//! Núcleo nativo do cliente desktop: bandeja, atalho global, cofre e IPC.

/// Entry point shared by `main.rs` and, later, by mobile targets.
pub fn run() -> tauri::Result<()> {
    tauri::Builder::default()
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_notification::init())
        .run(tauri::generate_context!())
}
