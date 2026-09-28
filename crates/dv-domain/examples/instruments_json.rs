//! Writes the score tables as JSON for the browser preview (apps/desktop/src/web/).
//! `cargo run -p dv-domain --example instruments_json > apps/desktop/src/web/instruments.json`

fn main() -> Result<(), serde_json::Error> {
    println!(
        "{}",
        serde_json::to_string_pretty(&dv_domain::instruments())?
    );
    Ok(())
}
