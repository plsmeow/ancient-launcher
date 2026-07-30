use crate::{
    app::options::Options,
    LAUNCHER_DIRECTORY
};

#[tauri::command]
pub(crate) async fn get_options() -> Result<Options, String> {
    let config_dir = LAUNCHER_DIRECTORY.config_dir();
    let options = Options::load(config_dir).await.unwrap_or_default();
    Ok(options)
}

#[tauri::command]
pub(crate) async fn store_options(options: Options) -> Result<(), String> {
    let config_dir = LAUNCHER_DIRECTORY.config_dir();
    options
        .store(config_dir)
        .await
        .map_err(|e| format!("unable to store config data: {:?}", e))?;
    Ok(())
}

#[tauri::command]
pub(crate) async fn default_data_folder_path() -> Result<String, String> {
    LAUNCHER_DIRECTORY.data_dir().to_str()
        .map(|s| s.to_string())
        .ok_or_else(|| "unable to get data folder path".to_string())
}
