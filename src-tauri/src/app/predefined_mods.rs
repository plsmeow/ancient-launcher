use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use tokio::fs;
use tracing::{info, warn};

use crate::HTTP_CLIENT;

const MODRINTH_API: &str = "https://api.modrinth.com/v2";
const USER_AGENT: &str = "ancient-launcher/1.0.0 (plsmeow)";

/// Definition of a pre-defined mod that can be toggled in the launcher UI.
#[derive(Clone, Debug, Serialize)]
pub struct PredefinedModDef {
    pub id: &'static str,
    pub name: &'static str,
    pub modrinth_slug: &'static str,
    pub depends_on: Option<&'static str>,
    pub always_enabled: bool,
}

/// User-controlled state of a pre-defined mod.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct PredefinedModState {
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Filename of the last downloaded artifact; used to remove it when disabled.
    pub filename: Option<String>,
}

fn default_true() -> bool {
    true
}

/// Full list of pre-defined mods shown in the UI.
pub const PREDEFINED_MODS: &[PredefinedModDef] = &[
    PredefinedModDef { id: "fabric-api", name: "Fabric API", modrinth_slug: "fabric-api", depends_on: None, always_enabled: true },
    PredefinedModDef { id: "sodium", name: "Sodium", modrinth_slug: "sodium", depends_on: None, always_enabled: false },
    PredefinedModDef { id: "sodium-extra", name: "Sodium Extra", modrinth_slug: "sodium-extra", depends_on: None, always_enabled: false },
    PredefinedModDef { id: "modmenu", name: "Mod Menu", modrinth_slug: "modmenu", depends_on: None, always_enabled: false },
    PredefinedModDef { id: "immediatelyfast", name: "ImmediatelyFast", modrinth_slug: "immediatelyfast", depends_on: None, always_enabled: false },
    PredefinedModDef { id: "ferrite-core", name: "Ferrite Core", modrinth_slug: "ferrite-core", depends_on: None, always_enabled: false },
    PredefinedModDef { id: "viafabricplus", name: "ViaFabricPlus", modrinth_slug: "viafabricplus", depends_on: None, always_enabled: false },
    PredefinedModDef { id: "cloth-config", name: "Cloth Config", modrinth_slug: "cloth-config", depends_on: None, always_enabled: true },
    PredefinedModDef { id: "moreculling", name: "More Culling", modrinth_slug: "moreculling", depends_on: None, always_enabled: false },
    PredefinedModDef { id: "entityculling", name: "Entity Culling", modrinth_slug: "entityculling", depends_on: None, always_enabled: false },
    PredefinedModDef { id: "network-protocol-disconnect", name: "Network Protocol Disconnect", modrinth_slug: "network-protocol-disconnect", depends_on: None, always_enabled: false },
    PredefinedModDef { id: "ukulib", name: "Ukulib", modrinth_slug: "ukulib", depends_on: None, always_enabled: false },
    PredefinedModDef { id: "ukus-armor-hud", name: "Uku's Armor HUD", modrinth_slug: "ukus-armor-hud", depends_on: Some("ukulib"), always_enabled: false },
    PredefinedModDef { id: "wider-tab", name: "Wider Tab", modrinth_slug: "wider-tab", depends_on: None, always_enabled: false },
];

#[derive(Deserialize)]
struct ModrinthVersion {
    game_versions: Vec<String>,
    loaders: Vec<String>,
    files: Vec<ModrinthFile>,
}

#[derive(Deserialize)]
struct ModrinthFile {
    filename: String,
    url: String,
    #[serde(default)]
    primary: bool,
}

/// Fetch the latest modrinth version compatible with fabric/1.21.4.
async fn fetch_modrinth_version(slug: &str) -> Result<ModrinthFile> {
    let url = format!("{}/project/{}/version", MODRINTH_API, slug);

    let versions = HTTP_CLIENT
        .get(&url)
        .header("User-Agent", USER_AGENT)
        .send()
        .await
        .with_context(|| format!("Failed to contact Modrinth for {}", slug))?
        .error_for_status()
        .with_context(|| format!("Modrinth returned non-2xx for {}", slug))?
        .json::<Vec<ModrinthVersion>>()
        .await
        .with_context(|| format!("Failed to parse Modrinth response for {}", slug))?;

    let compatible = versions.into_iter().find(|v| {
        v.game_versions.iter().any(|g| g == "1.21.4")
            && v.loaders.iter().any(|l| l == "fabric")
    });

    let version = compatible.ok_or_else(|| {
        anyhow::anyhow!("No compatible 1.21.4/fabric version found for {}", slug)
    })?;

    let mut iter = version.files.into_iter();
    iter.next()
        .ok_or_else(|| anyhow::anyhow!("Modrinth version for {} has no files", slug))
}

/// Download and install a single pre-defined mod. Returns the installed filename.
pub async fn install_predefined_mod(
    def: &PredefinedModDef,
    mods_path: &Path,
) -> Result<String> {
    let file = fetch_modrinth_version(def.modrinth_slug).await?;
    let dest = mods_path.join(&file.filename);

    info!("Downloading {} -> {}", def.name, file.filename);
    let bytes = crate::utils::download_file(&file.url, |_, _| {}).await?;
    fs::write(&dest, bytes).await
        .with_context(|| format!("Failed to write {}", dest.display()))?;

    Ok(file.filename)
}

/// Remove the file of a disabled pre-defined mod, if known.
pub async fn remove_predefined_mod(def: &PredefinedModDef, filename: Option<&str>, mods_path: &Path) -> Result<()> {
    match filename {
        Some(name) => {
            let path = mods_path.join(name);
            if path.exists() {
                warn!("Removing disabled mod {}", name);
                fs::remove_file(&path).await
                    .with_context(|| format!("Failed to remove {}", path.display()))?;
            }
        }
        None => {
            // Scan the directory for any file starting with the slug
            let mut entries = fs::read_dir(mods_path).await?;
            while let Some(entry) = entries.next_entry().await? {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.starts_with(def.modrinth_slug) && name.ends_with(".jar") {
                    warn!("Removing disabled mod {} (scanned)", name);
                    fs::remove_file(entry.path()).await
                        .with_context(|| format!("Failed to remove {}", entry.path().display()))?;
                }
            }
        }
    }
    Ok(())
}
