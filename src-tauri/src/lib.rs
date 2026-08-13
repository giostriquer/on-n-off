mod adapter;
mod backup;
mod claude;
mod cli;
mod codex;
mod commands;
mod config_io;
mod dto;
#[cfg(test)]
mod fake;
mod paths;
mod scanner;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(commands::AppState::production())
        .invoke_handler(tauri::generate_handler![
            commands::list_agents,
            commands::list_plugins,
            commands::set_plugin_enabled,
            commands::set_skill_enabled,
            commands::install_plugin,
            commands::uninstall_plugin,
            commands::refresh,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
