mod agent;
mod settings;
mod teams;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(agent::PendingWriteState::new())
        .manage(agent::ApprovalWaiter::new())
        .manage(agent::StreamGeneration::new())
        .manage(agent::LiveMode::new())
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
            agent::stream_message,
            agent::generate_title,
            agent::confirm_write,
            agent::cancel_stream,
            agent::set_mode,
            agent::check_claude_cli,
            agent::pick_folder,
            agent::fetch_local_models,
            settings::get_api_key,
            settings::set_api_key,
            settings::get_use_local_claude,
            settings::set_use_local_claude,
            settings::get_use_local_model,
            settings::set_use_local_model,
            settings::get_local_model_url,
            settings::set_local_model_url,
            settings::get_local_model_name,
            settings::set_local_model_name,
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
