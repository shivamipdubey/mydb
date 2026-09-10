//! The desktop shell (docs/02-architecture.md, component 1).
//!
//! The frontend never talks to a database driver. It reaches the backend only
//! through the commands registered here, which is what keeps
//! docs/17-coding-standards.md's "UI code never calls a database driver
//! directly" rule enforceable rather than merely conventional.

mod commands;
mod dto;
mod recording;
mod state;

/// Returns the roadmap phase this build implements.
#[tauri::command]
fn implemented_phase() -> u8 {
    mydb_core::IMPLEMENTED_PHASE
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    use tauri::Manager;

    let result = tauri::Builder::default()
        .manage(state::AppState::default())
        .setup(|app| {
            // Opened at startup rather than on the first write, so phase 1's
            // command history is imported once on launch and the audit log
            // viewer is not empty until the user happens to change something.
            // A failure here must not stop the app: the record store matters,
            // but not more than being able to run at all.
            match commands::open_records() {
                Ok(database) => match app.state::<state::AppState>().records.lock() {
                    Ok(mut guard) => *guard = Some(database),
                    Err(_) => log::error!("the local record store lock is poisoned at startup"),
                },
                Err(problem) => log::warn!("could not open the local record store: {problem}"),
            }

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
            implemented_phase,
            commands::list_connections,
            commands::save_connection,
            commands::delete_connection,
            commands::set_production_flag,
            commands::connect,
            commands::active_connection,
            commands::submit_command,
            commands::edit_command,
            commands::confirm_command,
            commands::cancel_command,
        ])
        .run(tauri::generate_context!());

    // Nothing above the shell can recover from the window failing to open, but
    // docs/17-coding-standards.md forbids panicking out of this layer, so the
    // failure is reported and the process exits cleanly instead.
    if let Err(error) = result {
        eprintln!("MYDB failed to start: {error}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_phase_one() {
        assert_eq!(implemented_phase(), 1);
    }
}
