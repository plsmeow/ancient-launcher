use anyhow::anyhow;
use std::sync::{Arc, Mutex};
use std::thread;
use tauri::{Emitter, Window};
use tracing::{error, info};

use crate::app::gui::{AppState, RunnerInstance, ShareableWindow};
use crate::app::options::Options;
use crate::minecraft::{
    auth::MinecraftAccount,
    launcher::{LauncherData, StartParameter},
    prelauncher,
    progress::ProgressUpdate,
};
use uuid::Uuid;

fn handle_stdout(window: &ShareableWindow, data: &[u8]) -> anyhow::Result<()> {
    let data = String::from_utf8(data.to_vec())?;
    if data.is_empty() {
        return Ok(());
    }
    info!("{}", data.strip_suffix("\n").unwrap_or(&data));
    window
        .lock()
        .map_err(|_| anyhow!("Window lock is poisoned"))?
        .emit("process-output", data)?;
    Ok(())
}

fn handle_stderr(window: &ShareableWindow, data: &[u8]) -> anyhow::Result<()> {
    let data = String::from_utf8(data.to_vec())?;
    if data.is_empty() {
        return Ok(());
    }
    error!("{}", data.strip_suffix("\n").unwrap_or(&data));
    window
        .lock()
        .map_err(|_| anyhow!("Window lock is poisoned"))?
        .emit("process-output", data)?;
    Ok(())
}

fn handle_progress(
    window: &ShareableWindow,
    progress_update: ProgressUpdate,
) -> anyhow::Result<()> {
    window
        .lock()
        .map_err(|_| anyhow!("Window lock is poisoned"))?
        .emit("progress-update", &progress_update)?;
    if let ProgressUpdate::SetLabel(label) = progress_update {
        handle_log(window, &label)?;
    }
    Ok(())
}

fn handle_log(window: &ShareableWindow, msg: &str) -> anyhow::Result<()> {
    info!("{}", msg);
    if let Ok(k) = window.lock() {
        let _ = k.emit("process-output", msg);
    }
    Ok(())
}

#[tauri::command]
pub(crate) async fn run_client(
    options: Options,
    window: Window,
    app_state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let shareable_window: ShareableWindow = Arc::new(Mutex::new(window));

    let minecraft_account = options
        .start_options
        .minecraft_account
        .ok_or("no account selected")?;

    let (account_name, uuid, token, user_type) = match &minecraft_account {
        MinecraftAccount::OfflineAccount { name, id } => {
            (name.clone(), id.to_string(), "-".to_string(), "legacy".to_string())
        }
        MinecraftAccount::MsaAccount { profile, mca, .. } => {
            (profile.name.clone(), profile.id.to_string(), mca.data.access_token.clone(), "msa".to_string())
        }
        MinecraftAccount::LegacyMsaAccount { name, uuid, token, .. } => {
            (name.clone(), uuid.to_string(), token.clone(), "msa".to_string())
        }
    };

    let xuid = Uuid::new_v4().to_string();

    // Capture predefined mod state before any partial moves of `options`.
    let predefined_mods_state = {
        let config_dir = crate::LAUNCHER_DIRECTORY.config_dir();
        crate::app::options::Options::load(config_dir)
            .await
            .map(|opts| opts.predefined_mods)
            .unwrap_or_else(|_| options.predefined_mods.clone())
    };

    let runner_instance = &app_state.runner_instance;
    if runner_instance
        .lock()
        .map_err(|e| format!("unable to lock runner instance: {:?}", e))?
        .is_some()
    {
        return Err("client is already running".to_string());
    }

    let (terminator_tx, terminator_rx) = tokio::sync::oneshot::channel();

    *runner_instance
        .lock()
        .map_err(|e| format!("unable to lock runner instance: {:?}", e))? = Some(RunnerInstance {
        terminator: terminator_tx,
    });

    let copy_of_runner_instance = runner_instance.clone();

    let parameters = StartParameter {
        java_distribution: options.start_options.java_distribution,
        jvm_args: vec![],
        memory: options.start_options.memory,
        custom_data_path: if !options.start_options.custom_data_path.is_empty() {
            Some(options.start_options.custom_data_path)
        } else {
            None
        },
        auth_player_name: account_name,
        auth_uuid: uuid,
        auth_access_token: token,
        auth_xuid: xuid,
        user_type,
        keep_launcher_open: options.launcher_options.keep_launcher_open,
        concurrent_downloads: options.launcher_options.concurrent_downloads,
    };

    let mods_options = predefined_mods_state;

    thread::spawn(move || {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                let keep_launcher_open = parameters.keep_launcher_open;

                let launcher_data = LauncherData {
                    on_stdout: handle_stdout,
                    on_stderr: handle_stderr,
                    on_progress: handle_progress,
                    on_log: handle_log,
                    hide_window: |w| w.lock().unwrap().hide().unwrap(),
                    data: Box::new(shareable_window.clone()),
                    terminator: terminator_rx,
                };

                if let Err(e) =
                    prelauncher::launch(parameters, launcher_data, &mods_options).await
                {
                    if !keep_launcher_open {
                        shareable_window.lock().unwrap().show().unwrap();
                    }
                    let message = format!("An error occurred:\n\n{:?}", e);
                    shareable_window
                        .lock()
                        .unwrap()
                        .emit("client-error", ())
                        .unwrap();
                    handle_stderr(&shareable_window, message.as_bytes()).unwrap();
                }

                *copy_of_runner_instance
                    .lock()
                    .map_err(|e| format!("unable to lock runner instance: {:?}", e))
                    .unwrap() = None;
                shareable_window
                    .lock()
                    .unwrap()
                    .emit("client-exited", ())
                    .unwrap()
            });
    });

    Ok(())
}

#[tauri::command]
pub(crate) async fn terminate(app_state: tauri::State<'_, AppState>) -> Result<(), String> {
    let mut lck = app_state
        .runner_instance
        .lock()
        .map_err(|e| format!("unable to lock runner instance: {:?}", e))?;

    if let Some(inst) = lck.take() {
        info!("Sending sigterm");
        inst.terminator.send(()).unwrap();
    }
    Ok(())
}

#[tauri::command]
pub(crate) async fn fetch_latest_release() -> Result<crate::app::github::GithubRelease, String> {
    crate::app::github::fetch_latest_release().await
}
