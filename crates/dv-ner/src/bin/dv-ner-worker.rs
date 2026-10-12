//! The name model worker on its own, for the tests (the app starts itself with the same
//! argument instead, `apps/desktop/src-tauri/src/main.rs`).

fn main() {
    let mut args = std::env::args().skip(1);
    let code = match (args.next().as_deref(), args.next()) {
        (Some(dv_ner::worker::WORKER_ARG), Some(dir)) => {
            dv_ner::worker::worker_main(std::path::Path::new(&dir))
        }
        _ => 2,
    };
    std::process::exit(code);
}
