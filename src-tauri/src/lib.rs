mod adapter;
mod antigravity;
mod backup;
mod claude;
mod cli;
mod cli_locate;
#[cfg(test)]
mod cli_stub;
mod codex;
mod commands;
mod config_io;
mod cursor;
mod dto;
#[cfg(test)]
mod fake;
mod flags;
mod github;
mod github_monitor;
mod http;
mod install_source;
mod item_install;
mod limits;
mod limits_monitor;
mod mcp;
mod monitor;
mod notifications;
mod paths;
mod plugin_meta;
mod process;
mod project;
mod scanner;
mod settings;
mod sort;
mod tray;
#[cfg(test)]
mod updater_build;
mod usage;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .manage(commands::AppState::production())
        .setup(|_app| {
            // Warm the CLI search path (login-shell PATH probe) off the UI thread so the
            // first provider load does not pay for it.
            std::thread::spawn(|| {
                let _ = cli_locate::cli_search_path();
            });
            #[cfg(target_os = "macos")]
            tray::setup(_app)?;
            limits_monitor::setup(_app);
            github_monitor::setup(_app);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_agents,
            commands::feature_flags,
            commands::updater_build_info,
            commands::load_app_settings,
            commands::save_app_settings,
            commands::request_notification_permission,
            commands::diagnose_providers,
            commands::list_projects,
            commands::inspect_project,
            commands::list_plugins,
            commands::list_local_plugins,
            commands::set_plugin_enabled,
            commands::set_skill_enabled,
            commands::set_mcp_enabled,
            commands::install_plugin,
            commands::uninstall_plugin,
            commands::update_plugin,
            commands::inspect_marketplace,
            commands::install_items,
            commands::item_update_status,
            commands::update_item,
            commands::remove_item,
            commands::open_url,
            commands::refresh,
            commands::usage_summary,
            commands::read_limits,
            commands::forget_limits_snapshot,
            commands::read_github_prs,
            commands::hide_limits_popover,
            commands::open_limits_window,
            commands::quit_app,
        ]);

    #[cfg(target_os = "macos")]
    let builder = builder.on_window_event(tray::handle_window_event);

    let app = builder
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    app.run(|_app, _event| {
        #[cfg(target_os = "macos")]
        if matches!(_event, tauri::RunEvent::Reopen { .. }) {
            if let Err(error) = tray::show_main_window(_app) {
                eprintln!("failed to reopen the main window: {error}");
            }
        }
    });
}
