pub mod commands;
pub mod crashlog;
pub mod dictionaries;
pub mod dictionary;
pub mod epub;
pub mod ext_commands;
pub mod extensions;
pub mod import;
pub mod native;
pub mod native_input;
pub mod store;
pub mod updater;

#[cfg(feature = "spikes")]
mod spikes;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        // D6: update checks run in the core (updater.rs); no page is granted the plugin.
        .plugin(tauri_plugin_updater::Builder::new().build())
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
        // §7.2, Spike G: extension packages, each on its own origin.
        .register_asynchronous_uri_scheme_protocol(
            extensions::EXT_SCHEME,
            |ctx, request, responder| {
                use tauri::Manager;
                let app = ctx.app_handle().clone();
                let id = request.uri().host().unwrap_or_default().to_string();
                let path = request.uri().path().to_string();
                std::thread::spawn(move || {
                    let state = app.state::<commands::AppState>();
                    responder.respond(extensions::serve(&state.library.extensions_dir, &id, &path));
                });
            },
        )
        // Reading Lens DX6: the reader's dictionaries, each generation on its own origin.
        .register_asynchronous_uri_scheme_protocol(
            dictionaries::serve::DICT_SCHEME,
            |ctx, request, responder| {
                use tauri::Manager;
                let app = ctx.app_handle().clone();
                let generation = request.uri().host().unwrap_or_default().to_string();
                let path = request.uri().path().to_string();
                let query = request.uri().query().unwrap_or_default().to_string();
                std::thread::spawn(move || {
                    let state = app.state::<commands::AppState>();
                    let dicts = app.state::<dictionaries::state::DictState>();
                    // Only an active generation, or one an open peek still uses (DX2).
                    let active = state
                        .store
                        .lock()
                        .unwrap()
                        .dictionaries()
                        .map(|d| d.iter().any(|d| d.generation == generation))
                        .unwrap_or(false);
                    let opened = (active || dicts.pinned(&generation))
                        .then(|| dicts.open(&generation))
                        .flatten();
                    responder.respond(dictionaries::serve::serve(opened.as_deref(), &path, &query));
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
                native::open_software_update,
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
                commands::startup_mark,
                commands::export_save_file,
                ext_commands::extensions_list,
                ext_commands::extension_inspect,
                ext_commands::extension_install,
                ext_commands::extension_set_enabled,
                ext_commands::extension_remove,
                ext_commands::extension_crashed,
                ext_commands::extension_restart,
                ext_commands::extension_storage_get,
                ext_commands::extension_storage_set,
                ext_commands::extension_storage_delete,
                ext_commands::extension_storage_keys,
                ext_commands::extension_net_fetch,
                ext_commands::app_safe_mode,
                ext_commands::app_restart_safe,
                commands::position_get,
                commands::setting_get,
                commands::setting_set,
                commands::opened_take,
                commands::quit_ready,
                commands::quit_saving,
                commands::quit_discard,
                native::screen_reader_running,
                native::preferred_languages,
                native::keyboard_layout_labels,
                native::screen_edges,
                native::set_window_controls,
                crashlog::crash_log_note,
                crashlog::crash_log_exists,
                crashlog::crash_log_show,
                updater::app_restart,
                updater::open_release_page,
                dictionary::look_up,
                dictionary::open_dictionary,
                dictionaries::commands::dict_list,
                dictionaries::commands::dict_import,
                dictionaries::commands::dict_cancel_import,
                dictionaries::commands::dict_remove,
                dictionaries::commands::dict_enable,
                dictionaries::commands::dict_reorder,
                dictionaries::commands::dict_lookup,
                dictionaries::commands::dict_release,
                ext_commands::extension_grant,
                ext_commands::extension_revoke,
                ext_commands::extension_secrets,
                ext_commands::extension_secret_has,
                ext_commands::extension_secret_delete,
                ext_commands::extension_secret_request,
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
        spikes::spike_corpus_dir,
        spikes::spike_install_ext,
        spikes::spike_install_unchecked,
        spikes::spike_read_file,
        spikes::spike_canary,
        spikes::spike_canary_log,
        spikes::spike_canary_clear,
        spikes::spike_report,
        spikes::spike_info,
        spikes::spike_log,
        spikes::spike_memory,
        spikes::spike_capture,
        spikes::spike_scroll_wheel,
        spikes::spike_mouse,
        spikes::spike_cursor,
        spikes::spike_scroller_style,
        spikes::spike_mouse_drag,
        spikes::spike_read_pasteboard,
        spikes::spike_crash_log,
        spikes::spike_capture_png,
        spikes::spike_dict_generations,
        spikes::spike_secret_save,
        spikes::spike_vault_deny,
        spikes::spike_vault_has,
        spikes::spike_bundle_languages,
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
            // D1: crashes go to a log on this Mac, nowhere else.
            let logs = crashlog::dir(app);
            crashlog::install_panic_hook(logs.clone());
            use tauri::Manager;
            app.manage(crashlog::CrashLog(logs));
            let state = commands::init(app)?;
            // P7: safe mode is decided before anything else reads the extensions.
            let safe = ext_commands::launch_in_safe_mode(&state);
            if let Err(e) = extensions::registry::ensure_builtins(
                &state.store.lock().unwrap(),
                &state.library.extensions_dir,
                ext_commands::BUILTINS,
            ) {
                log::error!("built-in extensions: {e}");
            }
            // DX2: interrupted imports and replaced generations go at launch.
            let dicts = dictionaries::state::DictState::new(state.library.dictionaries_dir.clone());
            dictionaries::commands::recover(&state, &dicts);
            app.manage(dicts);
            app.manage(ext_commands::Vaults::for_app());
            app.manage(state);
            app.manage(ext_commands::SafeMode(std::sync::atomic::AtomicBool::new(
                safe,
            )));
            commands::flush_pending_opens(app.handle());
            native_input::install(app.handle());
            updater::start(app.handle());
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
            tauri::RunEvent::ExitRequested { api, code, .. }
                if commands::hold_exit_for_save(app, code) =>
            {
                api.prevent_exit()
            }
            _ => {}
        });
}
