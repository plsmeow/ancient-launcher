use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Result};
use tokio::fs;
use tracing::*;

use crate::app::gui::ShareableWindow;
use crate::app::predefined_mods::{self, PredefinedModState, PREDEFINED_MODS};
use crate::error::LauncherError;
use crate::minecraft::launcher;
use crate::minecraft::launcher::{LauncherData, StartParameter};
use crate::minecraft::progress::{
    get_max, get_progress, ProgressReceiver, ProgressUpdate, ProgressUpdateSteps,
};
use crate::minecraft::version::{VersionManifest, VersionProfile};
use crate::utils::download_file;
use crate::LAUNCHER_DIRECTORY;

use backon::{ExponentialBuilder, Retryable};

const FABRIC_LOADER_VERSION: &str = "0.17.3";
const MC_VERSION: &str = "1.21.4";
const ANCIENT_JAR_NAME: &str = "onetap-1.0.0.jar";

pub(crate) async fn launch(
    launching_parameter: StartParameter,
    launcher_data: LauncherData<ShareableWindow>,
    predefined_mods_state: &HashMap<String, PredefinedModState>,
) -> Result<()> {
    launcher_data.progress_update(ProgressUpdate::set_max());
    launcher_data.progress_update(ProgressUpdate::SetProgress(0));
    launcher_data.progress_update(ProgressUpdate::set_label("Загрузка версии..."));

    let mc_version_manifest = VersionManifest::fetch
        .retry(ExponentialBuilder::default())
        .notify(|err, dur| {
            launcher_data.log(&format!(
                "Failed to load version manifest. Retrying in {:?}. Error: {}",
                dur, err
            ));
        })
        .await?;

    launcher_data.check_cancelled()?;

    let data_directory = launching_parameter
        .custom_data_path
        .clone()
        .map(|x| x.into())
        .unwrap_or_else(|| LAUNCHER_DIRECTORY.data_dir().to_path_buf());

    launcher_data.progress_update(ProgressUpdate::set_label("Загрузка Fabric..."));

    let manifest_url = format!(
        "https://meta.fabricmc.net/v2/versions/loader/{}/{}/profile/json",
        MC_VERSION, FABRIC_LOADER_VERSION
    );

    let mut version = (|| async { VersionProfile::load(&manifest_url).await })
        .retry(ExponentialBuilder::default())
        .notify(|err, dur| {
            launcher_data.log(&format!(
                "Failed to load Fabric profile. Retrying in {:?}. Error: {}",
                dur, err
            ));
        })
        .await?;

    if let Some(inherited_version) = &version.inherits_from {
        let url = mc_version_manifest
            .versions
            .iter()
            .find(|x| &x.id == inherited_version)
            .map(|x| &x.url)
            .ok_or_else(|| {
                LauncherError::InvalidVersionProfile(format!(
                    "unable to find inherited version manifest {}",
                    inherited_version
                ))
            })?;

        debug!(
            "Determined {}'s download url to be {}",
            inherited_version, url
        );
        launcher_data.log(&format!(
            "Downloading inherited version {}...",
            inherited_version
        ));

        let parent_version = (|| async { VersionProfile::load(url).await })
            .retry(ExponentialBuilder::default())
            .notify(|err, dur| {
                launcher_data.log(&format!(
                    "Failed to load inherited version profile: {}. Retrying in {:?}. Error: {}",
                    inherited_version, dur, err
                ));
            })
            .await?;
        version.merge(parent_version)?;
    }

    launcher_data.check_cancelled()?;

    setup_mods(&data_directory, predefined_mods_state, &launcher_data).await?;

    launcher_data.check_cancelled()?;

    launcher_data.progress_update(ProgressUpdate::set_label("Запуск..."));
    launcher::launch(
        &data_directory,
        version,
        launching_parameter,
        launcher_data,
    )
    .await?;
    Ok(())
}

/// Path to the directory containing the user's installed mods (fabric `mods` folder).
pub fn mods_directory(data: &Path) -> std::path::PathBuf {
    data.join("gameDir").join("mods")
}

async fn setup_mods(
    data: &Path,
    predefined_mods_state: &HashMap<String, PredefinedModState>,
    launcher_data: &LauncherData<ShareableWindow>,
) -> Result<()> {
    let mods_path = mods_directory(data);
    fs::create_dir_all(&mods_path).await
        .with_context(|| format!("Failed to create mods directory {}", mods_path.display()))?;

    // 1. Ancient client JAR (always required, downloaded from GitHub releases)
    install_ancient(&mods_path, launcher_data).await?;

    // 2. Predefined mods: download if enabled (or always_enabled), remove if disabled.
    //    Collect newly downloaded filenames so we can save them back to options.
    let mut new_filenames: HashMap<String, String> = HashMap::new();

    for def in PREDEFINED_MODS {
        if def.always_enabled {
            if let Some(fname) = install_mod_if_missing(def, &mods_path, launcher_data).await {
                new_filenames.insert(def.id.to_string(), fname);
            }
            continue;
        }

        let state = predefined_mods_state
            .get(def.id)
            .cloned()
            .unwrap_or_default();

        if !state.enabled {
            if let Err(e) = predefined_mods::remove_predefined_mod(
                def,
                state.filename.as_deref(),
                &mods_path,
            ).await {
                launcher_data.log(&format!(
                    "Не удалось удалить {}: {}",
                    def.name, e
                ));
            }
            continue;
        }

        if let Some(fname) = install_mod_if_missing(def, &mods_path, launcher_data).await {
            new_filenames.insert(def.id.to_string(), fname);
        }
    }

    // Save newly downloaded filenames back to options so get_custom_mods can filter them
    if !new_filenames.is_empty() {
        let config_dir = crate::LAUNCHER_DIRECTORY.config_dir();
        if let Ok(mut opts) = crate::app::options::Options::load(config_dir).await {
            for (id, fname) in &new_filenames {
                let state = opts.predefined_mods.entry(id.clone()).or_default();
                if state.filename.as_ref() != Some(fname) {
                    state.filename = Some(fname.clone());
                }
            }
            if let Err(e) = opts.store(config_dir).await {
                warn!("Failed to save mod filenames: {}", e);
            }
        }
    }

    Ok(())
}

async fn install_mod_if_missing(
    def: &predefined_mods::PredefinedModDef,
    mods_path: &Path,
    launcher_data: &LauncherData<ShareableWindow>,
) -> Option<String> {
    if let Some(existing) = find_mod_file(mods_path, def.modrinth_slug).await {
        launcher_data.log(&format!("{} уже установлен: {}", def.name, existing));
        return None;
    }

    launcher_data.progress_update(ProgressUpdate::set_label(&format!(
        "Скачивание {}...",
        def.name
    )));

    match predefined_mods::install_predefined_mod(def, mods_path).await {
        Ok(filename) => {
            launcher_data.log(&format!("{} установлен: {}", def.name, filename));
            Some(filename)
        }
        Err(e) => {
            launcher_data.log(&format!(
                "Не удалось установить {}: {}",
                def.name, e
            ));
            None
        }
    }
}

/// Scan the mods directory for a file whose name starts with the given slug.
async fn find_mod_file(mods_path: &Path, slug: &str) -> Option<String> {
    let mut entries = fs::read_dir(mods_path).await.ok()?;
    while let Ok(Some(entry)) = entries.next_entry().await {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with(slug) && name.ends_with(".jar") {
            return Some(name);
        }
    }
    None
}

async fn install_ancient(
    mods_path: &Path,
    launcher_data: &LauncherData<ShareableWindow>,
) -> Result<()> {
    launcher_data.progress_update(ProgressUpdate::set_label("Проверка обновлений Ancient..."));

    let release = crate::app::github::fetch_latest_release().await
        .map_err(|e| anyhow::anyhow!("Failed to fetch latest release: {}", e))?;

    let jar_url = crate::app::github::get_jar_url(&release)
        .ok_or_else(|| anyhow::anyhow!("onetap-1.0.0.jar not found in latest release"))?;

    let config_dir = crate::LAUNCHER_DIRECTORY.config_dir();
    let stored_tag = crate::app::options::Options::load(config_dir)
        .await
        .map(|o| o.ancient_version)
        .unwrap_or_default();

    let jar_dest = mods_path.join(ANCIENT_JAR_NAME);
    let needs_update = stored_tag != release.tag_name;

    if jar_dest.exists() && !needs_update {
        return Ok(());
    }

    if needs_update && jar_dest.exists() {
        launcher_data.log(&format!(
            "Обновление Ancient: {} -> {}",
            stored_tag, release.tag_name
        ));
        if let Err(e) = fs::remove_file(&jar_dest).await {
            warn!("Не удалось удалить старую версию Ancient: {}", e);
        }
    }

    launcher_data.progress_update(ProgressUpdate::set_label("Скачивание Ancient..."));
    launcher_data.log(&format!("Downloading Ancient from {}", jar_url));

    let bytes = download_file(jar_url, |a, b| {
        launcher_data.progress_update(ProgressUpdate::set_for_step(
            ProgressUpdateSteps::DownloadLiquidBounceMods,
            get_progress(0, a, b),
            get_max(1),
        ))
    })
    .await?;

    fs::write(&jar_dest, bytes).await
        .with_context(|| format!("Failed to write {}", ANCIENT_JAR_NAME))?;

    if let Ok(mut opts) = crate::app::options::Options::load(config_dir).await {
        opts.ancient_version = release.tag_name.clone();
        if let Err(e) = opts.store(config_dir).await {
            warn!("Не удалось сохранить версию Ancient: {}", e);
        }
    }

    launcher_data.log(&format!("Downloaded {} ({})", ANCIENT_JAR_NAME, release.tag_name));
    Ok(())
}
