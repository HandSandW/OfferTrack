//! Stable one-shot JSON CLI; intentionally separate from Tauri's desktop lifecycle.
use std::{
    ffi::OsString,
    io::{Read, Write},
    path::PathBuf,
};

use serde_json::{Value, json};

use crate::{
    agent_access,
    agent_access::reader::open,
    error::{AppErrorPayload, CoreError},
};

const MAX_INPUT: u64 = 64 * 1024;

fn request(args: Vec<OsString>, input: &mut impl Read) -> Result<Value, CoreError> {
    if args.len() == 1 && args[0] == "--help" {
        return Ok(
            json!({"usage": "offertrack-cli --warehouse <absolute-path> query < request.json",
            "input": {"version": 1, "request": {"operation": "list_applications", "scope": "all", "limit": 50}},
            "mcp_usage": "offertrack-cli --warehouse <absolute-path> mcp",
            "mcp_protocol_versions": crate::agent_mcp::PROTOCOL_VERSIONS,
            "contract": agent_access::describe()}),
        );
    }
    if args.len() != 3 || args[0] != "--warehouse" || (args[2] != "query" && args[2] != "write") {
        return Err(CoreError::Validation);
    }
    let mut bytes = Vec::new();
    input
        .take(MAX_INPUT + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| CoreError::Validation)?;
    if bytes.len() as u64 > MAX_INPUT {
        return Err(CoreError::AgentLimit);
    }
    let unique = crate::agent_mcp::parse_unique(&bytes)?;
    if args[2] == "write" {
        let request: crate::agent_write::Request =
            serde_json::from_value(unique).map_err(|_| CoreError::Validation)?;
        return crate::agent_write::execute(&PathBuf::from(&args[1]), &request, "cli");
    }
    // Reject trailing/multiple JSON values and unknown fields before opening any warehouse.
    let request: agent_access::Request =
        serde_json::from_value(unique).map_err(|_| CoreError::Validation)?;
    if request.version != agent_access::VERSION {
        return Err(CoreError::AgentVersion);
    }
    let reader = open(&PathBuf::from(&args[1]))?;
    agent_access::query(&reader.session, request)
}

/// Query mode emits one JSON envelope; MCP mode emits newline-delimited JSON-RPC.
/// Neither mode starts the desktop, writes logs, or listens on a port.
/// 0 = success; 2 = rejected/failed request; 3 = stdout unavailable.
pub fn run(
    args: impl IntoIterator<Item = OsString>,
    input: &mut impl Read,
    output: &mut impl Write,
) -> i32 {
    let args: Vec<_> = args.into_iter().collect();
    if args.len() == 3 && args[0] == "--warehouse" && args[2] == "mcp" {
        return crate::agent_mcp::run(&PathBuf::from(&args[1]), input, output);
    }
    let result = request(args, input).and_then(|data| {
        agent_access::encode(&json!({"version": agent_access::VERSION, "ok": true, "data": data}))
    });
    let (mut bytes, code) = match result {
        Ok(bytes) => (bytes, 0),
        Err(error) => (
            serde_json::to_vec(&json!({"version": agent_access::VERSION, "ok": false,
            "error": AppErrorPayload::from(error)}))
            .expect("fixed error DTO is serializable"),
            2,
        ),
    };
    bytes.push(b'\n');
    if output
        .write_all(&bytes)
        .and_then(|_| output.flush())
        .is_err()
    {
        3
    } else {
        code
    }
}

#[cfg(test)]
mod tests;
