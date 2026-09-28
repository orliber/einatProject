fn main() {
    // Declaring the app's commands makes Tauri require an explicit permission for
    // each one; `capabilities/main.json` grants exactly these to the main window.
    let attributes = tauri_build::Attributes::new()
        .app_manifest(tauri_build::AppManifest::new().commands(&["ping"]));
    if let Err(error) = tauri_build::try_build(attributes) {
        panic!("tauri-build failed: {error:#}");
    }
}
