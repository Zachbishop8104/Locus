mod claude;
mod settings;
mod teams;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Info)
                        .build(),
                )?;
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            claude::stream_message,
            claude::generate_title,
            claude::pick_folder,
            settings::get_api_key,
            settings::set_api_key,
            teams::start_teams_auth,
            teams::get_teams_status,
            teams::get_teams_credentials,
            teams::disconnect_teams,
            teams::get_teams_list,
            teams::get_team_channels,
            teams::get_channel_messages,
            teams::get_chats,
            teams::get_chat_messages,
            teams::send_chat_message,
            teams::send_channel_message,
            teams::get_user_photo,
            teams::open_url,
            teams::fetch_teams_image,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
