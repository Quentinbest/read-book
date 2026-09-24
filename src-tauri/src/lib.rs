pub mod commands;
pub mod epub;
pub mod import;
pub mod native;
pub mod native_input;
pub mod store;

#[cfg(feature = "spikes")]
mod spikes;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(commands::PendingOpens::default());
    #[cfg(feature = "spikes")]
    let builder = spikes::install(builder);
    // Spike builds serve the product commands too, so the in-app end-to-end tests
    // can drive the real app (macOS has no WebDriver for Tauri).
    macro_rules! handler {
        ($($extra:path),*) => {
            tauri::generate_handler![
                commands::library_list,
                commands::library_import,
                commands::book_bytes,
                commands::book_settings_get,
                commands::book_settings_set,
                commands::position_save,
                commands::position_get,
                commands::setting_get,
                commands::setting_set,
                commands::opened_take,
                native::screen_reader_running,
                native::keyboard_layout_labels,
                $($extra),*
            ]
        };
    }
    #[cfg(not(feature = "spikes"))]
    let builder = builder.invoke_handler(handler!());
    #[cfg(feature = "spikes")]
    let builder = builder.invoke_handler(handler!(
        spikes::spike_read_corpus,
        spikes::spike_corpus_path,
        spikes::spike_canary,
        spikes::spike_canary_log,
        spikes::spike_canary_clear,
        spikes::spike_report,
        spikes::spike_info,
        spikes::spike_log,
        spikes::spike_exit
    ));
    builder
        .setup(|app| {
            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Info)
                        .build(),
                )?;
            }
            let state = commands::init(app)?;
            use tauri::Manager;
            app.manage(state);
            commands::flush_pending_opens(app.handle());
            native_input::install(app.handle());
            #[cfg(feature = "spikes")]
            spikes::setup(app)?;
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            #[cfg(any(target_os = "macos", target_os = "ios"))]
            if let tauri::RunEvent::Opened { urls } = event {
                commands::handle_opened(app, urls);
            }
            #[cfg(not(any(target_os = "macos", target_os = "ios")))]
            let _ = (app, event);
        });
}
