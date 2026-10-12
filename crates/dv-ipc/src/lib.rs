//! Typed IPC contract between the desktop UI and the core.
//!
//! Every type that crosses the WebView boundary lives here and derives [`ts_rs::TS`],
//! so the TypeScript side is generated rather than written by hand. `cargo test`
//! regenerates `apps/desktop/src/ipc/generated/`; CI fails if the checked-in copy
//! is stale.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Version of the IPC contract. Bump on any breaking change to a command.
pub const IPC_VERSION: u32 = 1;

/// Reply to the `ping` command: proves the UI is talking to the Rust core.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PingResponse {
    pub ipc_version: u32,
    pub core_version: String,
    pub build_commit: String,
    /// True once the vault runs on the FIPS 140-3 validated module.
    pub fips_active: bool,
    pub platform: String,
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    fn sample() -> PingResponse {
        PingResponse {
            ipc_version: IPC_VERSION,
            core_version: "0.0.0".to_owned(),
            build_commit: "dev".to_owned(),
            fips_active: false,
            platform: "linux".to_owned(),
        }
    }

    #[test]
    fn ping_crosses_the_boundary_unchanged() {
        let json = serde_json::to_string(&sample()).unwrap();
        assert_eq!(
            serde_json::from_str::<PingResponse>(&json).unwrap(),
            sample()
        );
    }

    #[test]
    fn the_json_fields_are_exactly_the_ones_the_typescript_type_declares() {
        let value = serde_json::to_value(sample()).unwrap();
        let mut keys: Vec<&str> = value
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        // The type as ts-rs generates it (in memory: the export test rewrites the file on disk
        // while the tests run, and CI already checks the checked-in copy is current).
        let ts = <PingResponse as TS>::inline(&ts_rs::Config::default());
        // Field names as declared in the type: `name: type`, outside the doc comments.
        let code: String = ts
            .split("/**")
            .map(|part| part.split_once("*/").map_or(part, |(_, rest)| rest))
            .collect();
        let body = code.split_once('{').unwrap().1.rsplit_once('}').unwrap().0;
        let mut declared: Vec<&str> = body
            .split(',')
            .filter_map(|f| f.split_once(':').map(|(name, _)| name.trim()))
            .collect();
        declared.sort_unstable();
        assert_eq!(keys, declared);
    }

    #[test]
    fn a_reply_missing_a_field_is_refused_rather_than_filled_in() {
        let mut value = serde_json::to_value(sample()).unwrap();
        value.as_object_mut().unwrap().remove("fips_active");
        assert!(serde_json::from_value::<PingResponse>(value).is_err());
    }
}
