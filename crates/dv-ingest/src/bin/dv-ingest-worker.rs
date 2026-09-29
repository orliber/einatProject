//! Standalone worker (the app uses its own binary with `--dv-ingest-worker` instead).
//! Used by the tests to exercise the real process boundary.

fn main() {
    std::process::exit(dv_ingest::worker::worker_main());
}
