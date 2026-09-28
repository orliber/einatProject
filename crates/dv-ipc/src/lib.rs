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
