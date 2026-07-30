use tauri::{Emitter, Window};
use tracing::{debug, info};

use crate::minecraft::auth::MinecraftAccount;

#[tauri::command]
pub(crate) async fn login_offline(username: &str) -> Result<MinecraftAccount, String> {
    let account = MinecraftAccount::auth_offline(username.to_string()).await;
    Ok(account)
}

#[tauri::command]
pub(crate) async fn login_microsoft(window: Window) -> Result<MinecraftAccount, String> {
    let account = MinecraftAccount::auth_msa(|uri, code| {
        debug!("enter code {} on {} to sign-in", code, uri);
        let _ = window.emit("microsoft_code", serde_json::json!({
            "uri": uri,
            "code": code
        }));
    })
        .await
        .map_err(|e| format!("{}", e))?;

    Ok(account)
}

#[tauri::command]
pub(crate) async fn refresh(account_data: MinecraftAccount) -> Result<MinecraftAccount, String> {
    info!("Refreshing account...");
    let account = account_data
        .refresh()
        .await
        .map_err(|e| format!("unable to refresh: {:?}", e))?;
    info!(
        "Account was refreshed - username {}",
        account.get_username()
    );
    Ok(account)
}

#[tauri::command]
pub(crate) async fn logout(account_data: MinecraftAccount) -> Result<(), String> {
    account_data
        .logout()
        .await
        .map_err(|e| format!("unable to logout: {:?}", e))
}
