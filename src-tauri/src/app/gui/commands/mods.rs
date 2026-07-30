use std::path::PathBuf;

use serde::Serialize;
use tracing::info;

use crate::app::options::Options;
use crate::app::predefined_mods::{self, PredefinedModDef, PredefinedModState};
use crate::LAUNCHER_DIRECTORY;

#[derive(Serialize)]
pub struct PredefinedModInfo {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    pub installed: bool,
}

#[derive(Serialize)]
pub struct CustomModInfo {
    pub filename: String,
    pub enabled: bool,
}

fn resolve_data_path(options: &Options) -> PathBuf {
    if !options.start_options.custom_data_path.is_empty() {
        PathBuf::from(&options.start_options.custom_data_path)
    } else {
        LAUNCHER_DIRECTORY.data_dir().to_path_buf()
    }
}

fn mods_path(options: &Options) -> PathBuf {
    crate::minecraft::prelauncher::mods_directory(&resolve_data_path(options))
}

async fn load_disk_predefined_mods() -> std::collections::HashMap<String, PredefinedModState> {
    let config_dir = LAUNCHER_DIRECTORY.config_dir();
    if let Ok(opts) = Options::load(config_dir).await {
        return opts.predefined_mods;
    }
    std::collections::HashMap::new()
}

#[tauri::command]
pub(crate) async fn get_predefined_mods(options: Options) -> Result<Vec<PredefinedModInfo>, String> {
    let mods = mods_path(&options);
    let disk_mods = load_disk_predefined_mods().await;

    let hidden_ids: &[&str] = &["fabric-api", "ukulib", "cloth-config"];

    let parent_enabled = |def: &PredefinedModDef| -> bool {
        match def.depends_on {
            Some(parent_id) => {
                if hidden_ids.contains(&parent_id) {
                    return true;
                }
                disk_mods
                    .get(parent_id)
                    .map(|s| s.enabled)
                    .unwrap_or(false)
            }
            None => true,
        }
    };

    let result = predefined_mods::PREDEFINED_MODS
        .iter()
        .filter(|def| !hidden_ids.contains(&def.id))
        .filter(|def| parent_enabled(def))
        .map(|def| {
            let state = disk_mods.get(def.id).cloned().unwrap_or_default();

            let installed = if let Some(ref fname) = state.filename {
                mods.join(fname).exists()
            } else {
                // Scan directory for file matching the mod slug
                std::fs::read_dir(&mods)
                    .ok()
                    .and_then(|entries| {
                        entries.filter_map(|e| e.ok()).find(|e| {
                            let name = e.file_name().to_string_lossy().to_string();
                            name.starts_with(def.modrinth_slug)
                                && (name.ends_with(".jar") || name.ends_with(".jar.disabled"))
                        })
                    })
                    .is_some()
            };

            PredefinedModInfo {
                id: def.id.to_string(),
                name: def.name.to_string(),
                enabled: state.enabled,
                installed,
            }
        })
        .collect();
    Ok(result)
}

#[tauri::command]
pub(crate) async fn set_predefined_mod_enabled(
    mut options: Options,
    id: String,
    enabled: bool,
) -> Result<std::collections::HashMap<String, PredefinedModState>, String> {
    let state = options
        .predefined_mods
        .entry(id.clone())
        .or_default();
    state.enabled = enabled;

    // Cascade toggles to dependent mods (forward)
    for dep in predefined_mods::PREDEFINED_MODS.iter() {
        if dep.depends_on == Some(id.as_str()) {
            let dep_state = options.predefined_mods.entry(dep.id.to_string()).or_default();
            dep_state.enabled = enabled;
            info!("Cascading {} -> {} ({})", id, dep.id, enabled);
        }
    }

    // Backward cascade: also toggle the dependency of this mod
    if let Some(def) = predefined_mods::PREDEFINED_MODS.iter().find(|d| d.id == id.as_str()) {
        if let Some(dep_id) = def.depends_on {
            let dep_state = options.predefined_mods.entry(dep_id.to_string()).or_default();
            dep_state.enabled = enabled;
            info!("Backward cascade {} -> {} ({})", id, dep_id, enabled);
        }
    }

    let config_dir = LAUNCHER_DIRECTORY.config_dir();
    options
        .store(config_dir)
        .await
        .map_err(|e| format!("unable to store options: {:?}", e))?;
    info!("Predefined mod {} -> {}", id, enabled);
    Ok(options.predefined_mods)
}

#[tauri::command]
pub(crate) async fn get_custom_mods(options: Options) -> Result<Vec<CustomModInfo>, String> {
    let mods = mods_path(&options);
    if !mods.exists() {
        return Ok(Vec::new());
    }

    let disk_mods = load_disk_predefined_mods().await;
    let known: std::collections::HashSet<String> = predefined_mods::PREDEFINED_MODS
        .iter()
        .filter_map(|def| disk_mods.get(def.id).and_then(|s| s.filename.clone()))
        .collect();

    // Build list of predefined slugs for prefix matching
    let predefined_slugs: Vec<&str> = predefined_mods::PREDEFINED_MODS
        .iter()
        .map(|def| def.modrinth_slug)
        .collect();

    let is_predefined = |fname: &str| -> bool {
        if known.contains(fname) {
            return true;
        }
        for slug in &predefined_slugs {
            if let Some(rest) = fname.strip_prefix(slug) {
                if rest.is_empty() || !rest.starts_with(|c: char| c.is_alphanumeric()) {
                    return true;
                }
            }
        }
        false
    };

    let mut entries = tokio::fs::read_dir(&mods)
        .await
        .map_err(|e| format!("read_dir failed: {}", e))?;

    let mut result = Vec::new();
    while let Some(entry) = entries
        .next_entry()
        .await
        .map_err(|e| format!("next_entry: {}", e))?
    {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(filename) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if filename == "onetap-1.0.0.jar" {
            continue;
        }

        if filename.ends_with(".jar") {
            if is_predefined(filename) {
                continue;
            }
            result.push(CustomModInfo {
                filename: filename.to_string(),
                enabled: true,
            });
        } else if filename.ends_with(".jar.disabled") {
            let original = filename.trim_end_matches(".disabled").to_string();
            if is_predefined(&original) {
                continue;
            }
            result.push(CustomModInfo {
                filename: original,
                enabled: false,
            });
        }
    }

    result.sort_by(|a, b| a.filename.cmp(&b.filename));
    Ok(result)
}

#[tauri::command]
pub(crate) async fn toggle_custom_mod(
    options: Options,
    filename: String,
    enabled: bool,
) -> Result<(), String> {
    let mods = mods_path(&options);
    let active_path = mods.join(&filename);
    let disabled_path = mods.join(format!("{}.disabled", filename));

    if enabled {
        if disabled_path.exists() {
            tokio::fs::rename(&disabled_path, &active_path)
                .await
                .map_err(|e| format!("rename failed: {}", e))?;
            info!("Enabled custom mod {}", filename);
        }
    } else {
        if active_path.exists() {
            tokio::fs::rename(&active_path, &disabled_path)
                .await
                .map_err(|e| format!("rename failed: {}", e))?;
            info!("Disabled custom mod {}", filename);
        }
    }
    Ok(())
}

#[tauri::command]
pub(crate) async fn install_custom_mod(
    options: Options,
    source_path: String,
) -> Result<String, String> {
    let src = PathBuf::from(&source_path);
    if !src.is_file() {
        return Err(format!("Файл не найден: {}", source_path));
    }
    let filename = src
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| "некорректное имя файла".to_string())?
        .to_string();

    let dest = mods_path(&options).join(&filename);
    tokio::fs::create_dir_all(dest.parent().unwrap())
        .await
        .map_err(|e| format!("create_dir_all: {}", e))?;
    tokio::fs::copy(&src, &dest)
        .await
        .map_err(|e| format!("copy failed: {}", e))?;
    info!("Installed custom mod {}", filename);
    Ok(filename)
}

#[tauri::command]
pub(crate) async fn delete_custom_mod(
    options: Options,
    filename: String,
) -> Result<(), String> {
    let mods = mods_path(&options);
    let active_path = mods.join(&filename);
    let disabled_path = mods.join(format!("{}.disabled", filename));

    if active_path.exists() {
        tokio::fs::remove_file(&active_path)
            .await
            .map_err(|e| format!("remove_file: {}", e))?;
        info!("Removed custom mod {}", filename);
    }
    if disabled_path.exists() {
        tokio::fs::remove_file(&disabled_path)
            .await
            .map_err(|e| format!("remove_file: {}", e))?;
        info!("Removed custom mod {} (disabled)", filename);
    }
    Ok(())
}
