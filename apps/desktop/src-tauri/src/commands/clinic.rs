//! The clinic (beta, D-170): one command for everything its screens ask, so the module sits
//! behind one door and each allowlist of the shell grows by one line.

use dv_domain::clinic::{ClinicReply, ClinicRequest};

use crate::*;

/// Every request of the clinic screens. Nothing of it leaves the computer: the core's
/// clinic service knows the vault and the rules only.
#[tauri::command]
pub(crate) async fn clinic(
    state: tauri::State<'_, AppState>,
    request: ClinicRequest,
) -> Res<ClinicReply> {
    with_core(&state, move |c| c.clinic(request)).await
}
