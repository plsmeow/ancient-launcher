use std::path::Path;
use std::collections::HashMap;

use crate::minecraft::java::DistributionSelection;
use crate::minecraft::auth::MinecraftAccount;
use crate::app::predefined_mods::PredefinedModState;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use tokio::fs;
use tracing::info;

#[derive(Serialize, Deserialize)]
pub(crate) struct Options {
    #[serde(rename = "start")]
    pub start_options: StartOptions,
    #[serde(rename = "launcher")]
    pub launcher_options: LauncherOptions,
    #[serde(rename = "predefinedMods", default)]
    pub predefined_mods: HashMap<String, PredefinedModState>,
    #[serde(rename = "ancientVersion", default)]
    pub ancient_version: String,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct StartOptions {
    #[serde(rename = "account")]
    pub minecraft_account: Option<MinecraftAccount>,
    #[serde(rename = "customDataPath", default)]
    pub custom_data_path: String,
    #[serde(rename = "javaDistribution", default)]
    pub java_distribution: DistributionSelection,
    #[serde(rename = "memory", default = "default_memory")]
    pub memory: u64,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct LauncherOptions {
    #[serde(rename = "concurrentDownloads")]
    pub concurrent_downloads: u32,
    #[serde(rename = "keepLauncherOpen")]
    pub keep_launcher_open: bool,
}

impl Options {
    pub async fn load(app_data: &Path) -> Result<Self> {
        let file_content = fs::read(app_data.join("options.json")).await?;
        if let Ok(options) = serde_json::from_slice::<Self>(&file_content) {
            info!("Successfully loaded options from file");
            return Ok(options);
        }
        Ok(serde_json::from_slice::<Self>(&file_content)?)
    }

    pub async fn store(&self, app_data: &Path) -> Result<()> {
        fs::write(app_data.join("options.json"), serde_json::to_string(&self)?).await?;
        Ok(())
    }
}

impl Default for StartOptions {
    fn default() -> Self {
        Self {
            minecraft_account: None,
            java_distribution: DistributionSelection::default(),
            custom_data_path: String::new(),
            memory: 4096,
        }
    }
}

impl Default for LauncherOptions {
    fn default() -> Self {
        Self {
            concurrent_downloads: 10,
            keep_launcher_open: false,
        }
    }
}

impl Default for Options {
    fn default() -> Self {
        Self {
            start_options: StartOptions::default(),
            launcher_options: LauncherOptions::default(),
            predefined_mods: HashMap::new(),
            ancient_version: String::new(),
        }
    }
}

fn default_memory() -> u64 {
    4096
}
