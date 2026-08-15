mod adapter;
mod antigravity;
mod backup;
mod claude;
mod cli;
mod codex;
mod commands;
mod config_io;
mod dto;
#[cfg(test)]
mod fake;
mod flags;
mod install_source;
mod mcp;
mod paths;
mod plugin_meta;
mod project;
mod scanner;
mod settings;
mod sort;
#[cfg(test)]
mod updater_build;
mod usage;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .manage(commands::AppState::production())
        .invoke_handler(tauri::generate_handler![
            commands::list_agents,
            commands::feature_flags,
            commands::updater_build_info,
            commands::load_app_settings,
            commands::save_app_settings,
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
            commands::refresh,
            commands::usage_summary,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
