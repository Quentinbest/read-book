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
        // §6.4: book media straight from the zip, never as blobs (see commands::BOOK_SCHEME).
        .register_asynchronous_uri_scheme_protocol(
            commands::BOOK_SCHEME,
            |ctx, request, responder| {
                use tauri::Manager;
                let app = ctx.app_handle().clone();
                let path = request.uri().path().to_string();
                std::thread::spawn(move || {
                    let state = app.state::<commands::AppState>();
                    responder.respond(commands::serve_book_media(&state, &path));
                });
            },
        )
        .manage(commands::PendingOpens::default())
        .manage(commands::QuitState::default());
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
                commands::book_entries,
                commands::book_entry,
                commands::book_damage,
                commands::annotations_list,
                commands::annotation_save,
                commands::annotation_delete,
                commands::annotation_restore,
                commands::search_text_get,
                commands::search_text_put,
                native::open_external,
                native::copy_text,
                commands::book_settings_get,
                commands::book_settings_set,
                commands::position_save,
                commands::library_remove,
                commands::library_restore,
                commands::book_show_file,
                commands::book_info,
                commands::library_folder,
                commands::library_folder_show,
                commands::export_write,
                commands::position_get,
                commands::setting_get,
                commands::setting_set,
                commands::opened_take,
                commands::quit_ready,
                native::screen_reader_running,
                native::keyboard_layout_labels,
                native::screen_edges,
                native::set_window_controls,
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
        spikes::spike_memory,
        spikes::spike_capture,
        spikes::spike_scroll_wheel,
        spikes::spike_read_pasteboard,
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
        .run(|app, event| match event {
            #[cfg(any(target_os = "macos", target_os = "ios"))]
            tauri::RunEvent::Opened { urls } => commands::handle_opened(app, urls),
            // Hold the first exit request so the reader can save (N5).
            tauri::RunEvent::ExitRequested { api, .. } if commands::hold_exit_for_save(app) => {
                api.prevent_exit()
            }
            _ => {}
        });
}
