use tauri::Emitter;

mod notes;
pub mod search;
pub mod watcher;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            // Lets the workspace watcher push note-list deltas to the window
            // (ADR-009) without every command taking an `AppHandle`.
            let handle = app.handle().clone();
            watcher::set_emitter(std::sync::Arc::new(move |delta| {
                let _ = handle.emit(watcher::WORKSPACE_EVENT, delta);
            }));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            notes::list_notes,
            notes::read_note,
            notes::write_note,
            notes::create_note,
            notes::delete_note,
            notes::restore_note,
            notes::rename_note,
            notes::search_notes,
            notes::search_cancel,
            notes::snapshot_status
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
