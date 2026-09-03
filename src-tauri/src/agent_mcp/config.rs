//! Connection instructions only. Never launches a process or writes client config.
use crate::{error::CoreError, filesystem};
use serde::Serialize;
use serde_json::{Value, json};
use std::path::Path;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Connection {
    version: u32,
    cli_available: bool,
    configuration: Value,
    protocol_versions: [&'static str; 2],
}

pub(crate) fn connection(executable: &Path, warehouse: &Path) -> Result<Connection, CoreError> {
    let parent = executable.parent().ok_or(CoreError::FileMissing)?;
    let cli = parent.join(if cfg!(windows) {
        "offertrack-cli.exe"
    } else {
        "offertrack-cli"
    });
    let cli_available = std::fs::symlink_metadata(&cli)
        .is_ok_and(|m| m.is_file() && !filesystem::is_reparse_point(&m));
    Ok(Connection {
        version: 1,
        cli_available,
        configuration: json!({"mcpServers":{"offertrack":{
            "command":cli.to_string_lossy(),"args":["--warehouse",warehouse.to_string_lossy(),"mcp"]}}}),
        protocol_versions: super::PROTOCOL_VERSIONS,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn configuration_preserves_argument_boundaries_and_does_not_create_or_execute_files() {
        let temp = tempfile::tempdir().unwrap();
        let exe = temp.path().join("offertrack.exe");
        let warehouse = temp.path().join("求职 仓库 & echo synthetic");
        let absent = connection(&exe, &warehouse).unwrap();
        assert!(!absent.cli_available);
        assert_eq!(std::fs::read_dir(temp.path()).unwrap().count(), 0);
        let command = absent.configuration["mcpServers"]["offertrack"]["command"]
            .as_str()
            .unwrap();
        std::fs::write(command, b"synthetic; not an executable").unwrap();
        let present = connection(&exe, &warehouse).unwrap();
        assert!(present.cli_available);
        assert_eq!(
            present.configuration["mcpServers"]["offertrack"]["args"],
            json!(["--warehouse", warehouse.to_string_lossy(), "mcp"])
        );
        assert!(!warehouse.exists());
    }
}
