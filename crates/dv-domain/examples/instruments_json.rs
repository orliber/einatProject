//! Writes the score tables as JSON for the browser preview:
//! `cargo run -p dv-domain --example instruments_json -- apps/desktop/src/web/instruments.json`

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "apps/desktop/src/web/instruments.json".into());
    let mut json = serde_json::to_string_pretty(&dv_domain::instruments())?;
    json.push('\n');
    std::fs::write(path, json)?;
    Ok(())
}
