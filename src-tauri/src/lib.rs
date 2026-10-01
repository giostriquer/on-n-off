mod accounts;
mod adapter;
mod antigravity;
mod backup;
mod claude;
mod claude_mcp;
mod cli;
mod cli_locate;
#[cfg(test)]
mod cli_stub;
mod codex;
mod commands;
mod config_io;
mod cursor;
mod dto;
mod file_lease;
mod flags;
mod github;
mod github_monitor;
mod hooks;
mod http;
mod install_source;
mod item_install;
mod limits;
mod limits_monitor;
mod limits_refresh;
mod mcp;
mod monitor;
mod notifications;
mod paths;
mod plugin_files;
mod plugin_meta;
mod process;
mod project;
mod read_revision;
mod scanner;
mod settings;
mod sha;
mod side_notch;
mod sort;
mod subscription;
mod tray;
#[cfg(test)]
mod updater_build;
mod usage;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default();
    #[cfg(all(target_os = "windows", feature = "single-instance"))]
    let builder = builder.plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
        if let Err(error) = tray::show_main_window(app) {
            eprintln!("failed to raise the running instance: {error}");
        }
    }));
    let builder = builder
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .manage(commands::AppState::production())
        .setup(|_app| {
            read_revision::register(_app.handle());
            std::thread::spawn(|| {
                let _ = cli_locate::cli_search_path();
                if let Ok(home) = paths::user_home() {
                    subscription::discard_legacy_cache(&home);
                }
            });
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            tray::setup(_app)?;
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            side_notch::setup(_app);
            #[cfg(target_os = "macos")]
            notifications::refresh_authorization(_app.handle().clone());
            limits_monitor::setup(_app);
            accounts::discovery::setup(_app);
            github_monitor::setup(_app);
            usage::spawn_history_folding();
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
            commands::usage_history_status,
            commands::clear_usage_history,
            commands::read_limits,
            commands::read_accounts,
            commands::read_account_preferences,
            commands::read_account_activation_blockers,
            commands::account_action,
            commands::add_account,
            commands::cancel_account_login,
            commands::read_codex_subscription,
            commands::forget_limits_snapshot,
            commands::set_limits_archived,
            commands::consume_codex_reset_credit,
            commands::pending_reset_spends,
            commands::cancel_reset_spend,
            commands::read_github_prs,
            commands::hide_limits_popover,
            commands::open_limits_window,
            commands::quit_app,
            commands::tray_supported,
            commands::read_notch_state,
            commands::save_notch_settings,
        ]);

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    let builder = builder.on_window_event(|window, event| {
        tray::handle_window_event(window, event);
    });

    let app = builder
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    app.run(|_app, _event| {
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        if matches!(_event, tauri::RunEvent::Exit) {
            side_notch::shutdown(_app);
        }
        #[cfg(target_os = "macos")]
        if matches!(_event, tauri::RunEvent::Reopen { .. }) {
            if let Err(error) = tray::show_main_window(_app) {
                eprintln!("failed to reopen the main window: {error}");
            }
        }
    });
}
