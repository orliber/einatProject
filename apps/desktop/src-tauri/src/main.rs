//! Desktop shell. Every command is a one-line delegation to `dv-core`.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[tauri::command]
fn ping() -> dv_ipc::PingResponse {
    dv_core::ping()
}

fn main() {
    let result = tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![ping])
        .run(tauri::generate_context!());
    if result.is_err() {
        std::process::exit(1);
    }
}
