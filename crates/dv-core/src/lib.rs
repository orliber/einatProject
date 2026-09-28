//! Application services used by the desktop shell.
//!
//! The Tauri commands are thin wrappers around the functions here, so all behavior
//! is testable on any machine without a WebView.

use dv_ipc::{PingResponse, IPC_VERSION};

/// Answer the UI's liveness check.
#[must_use]
pub fn ping() -> PingResponse {
    PingResponse {
        ipc_version: IPC_VERSION,
        core_version: env!("CARGO_PKG_VERSION").to_owned(),
        build_commit: option_env!("DV_BUILD_COMMIT").unwrap_or("dev").to_owned(),
        fips_active: false,
        platform: std::env::consts::OS.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ping_reports_contract_version_and_platform() {
        let reply = ping();
        assert_eq!(reply.ipc_version, IPC_VERSION);
        assert_eq!(reply.core_version, env!("CARGO_PKG_VERSION"));
        assert!(!reply.platform.is_empty());
    }
}
