use std::{fs, path::PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::CoreError;

const PREFERENCES_FILE: &str = "startup.json";

#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct StartupPreferences {
    remembered_warehouse_path: Option<PathBuf>,
}

pub fn load_remembered_warehouse(config_dir: PathBuf) -> Option<PathBuf> {
    let bytes = fs::read(config_dir.join(PREFERENCES_FILE)).ok()?;
    serde_json::from_slice::<StartupPreferences>(&bytes)
        .ok()?
        .remembered_warehouse_path
}

pub fn remember_warehouse(config_dir: PathBuf, path: PathBuf) -> Result<(), CoreError> {
    fs::create_dir_all(&config_dir).map_err(|_| CoreError::Storage)?;
    let preferences = StartupPreferences {
        remembered_warehouse_path: Some(path),
    };
    let bytes = serde_json::to_vec_pretty(&preferences).map_err(|_| CoreError::Storage)?;
    fs::write(config_dir.join(PREFERENCES_FILE), bytes).map_err(|_| CoreError::Storage)
}
