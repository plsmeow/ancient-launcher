use crate::{utils, LAUNCHER_VERSION};

#[tauri::command]
pub(crate) async fn get_launcher_version() -> Result<String, String> {
    Ok(LAUNCHER_VERSION.to_string())
}

#[tauri::command]
pub(crate) fn sys_memory() -> u64 {
    utils::sys_memory() / (1024 * 1024)
}
