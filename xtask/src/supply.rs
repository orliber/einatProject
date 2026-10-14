//! The supply chain, checked on this computer.
//!
//! * `cargo xtask check-parsers` (SEC7) – every version of a crate that reads input from
//!   outside must be pinned, with its checksum, in `supply-chain/parsers.txt`. A new version
//!   fails until someone reads its changes and adds a line.
//! * `cargo xtask sbom <out.json>` (SEC6) – a CycloneDX 1.5 list of everything that goes
//!   into the program: the Rust crates of `Cargo.lock` and the packages of `pnpm-lock.yaml`,
//!   with their checksums. No time stamp, so the same tree gives the same file.
//! * `cargo xtask self-audit [--full]` (SEC8) – the checks a release needs, in one command.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use std::process::Command;

const PARSERS_FILE: &str = "supply-chain/parsers.txt";
const PUBLIC_FILE: &str = "crates/dv-egress/update_key.pub";

/// A package in `Cargo.lock`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Locked {
    pub name: String,
    pub version: String,
    /// `None` for the workspace's own crates.
    pub source: Option<String>,
    pub checksum: Option<String>,
}

fn unquote(v: &str) -> String {
    v.trim().trim_matches('"').to_owned()
}

/// The `[[package]]` entries of a `Cargo.lock`.
#[must_use]
pub fn parse_cargo_lock(text: &str) -> Vec<Locked> {
    let mut out = Vec::new();
    let mut cur: Option<Locked> = None;
    for line in text.lines() {
        if line.trim() == "[[package]]" {
            out.extend(cur.take());
            cur = Some(Locked {
                name: String::new(),
                version: String::new(),
                source: None,
                checksum: None,
            });
            continue;
        }
        if line.starts_with('[') {
            out.extend(cur.take());
            continue;
        }
        let (Some(p), Some((k, v))) = (cur.as_mut(), line.split_once(" = ")) else {
            continue;
        };
        match k.trim() {
            "name" => p.name = unquote(v),
            "version" => p.version = unquote(v),
            "source" => p.source = Some(unquote(v)),
            "checksum" => p.checksum = Some(unquote(v)),
            _ => {}
        }
    }
    out.extend(cur);
    out
}

/// The watched crates and the pinned (name, version, checksum) lines.
fn parse_pins(text: &str) -> (BTreeSet<String>, BTreeSet<(String, String, String)>) {
    let mut watch = BTreeSet::new();
    let mut pins = BTreeSet::new();
    for line in text.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("# watch:") {
            watch.extend(rest.split_whitespace().map(str::to_owned));
            continue;
        }
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let f: Vec<&str> = line.split_whitespace().collect();
        if let [name, version, checksum] = f[..] {
            pins.insert((name.to_owned(), version.to_owned(), checksum.to_owned()));
        }
    }
    (watch, pins)
}

/// What in `lock` is not pinned in `pins_text`.
#[must_use]
pub fn parser_problems(lock: &[Locked], pins_text: &str) -> Vec<String> {
    let (watch, pins) = parse_pins(pins_text);
    if watch.is_empty() {
        return vec![format!("{PARSERS_FILE}: no `# watch:` line")];
    }
    let mut problems = Vec::new();
    for p in lock.iter().filter(|p| watch.contains(&p.name)) {
        let Some(sum) = &p.checksum else {
            problems.push(format!(
                "{} {}: not from crates.io (no checksum), cannot be pinned",
                p.name, p.version
            ));
            continue;
        };
        if !pins.contains(&(p.name.clone(), p.version.clone(), sum.clone())) {
            problems.push(format!(
                "{} {} ({sum}) is not in {PARSERS_FILE}: read what changed in it, then add the line",
                p.name, p.version
            ));
        }
    }
    problems
}

pub fn check_parsers(root: &Path) -> Result<Vec<String>, String> {
    let lock =
        fs::read_to_string(root.join("Cargo.lock")).map_err(|e| format!("Cargo.lock: {e}"))?;
    let pins =
        fs::read_to_string(root.join(PARSERS_FILE)).map_err(|e| format!("{PARSERS_FILE}: {e}"))?;
    Ok(parser_problems(&parse_cargo_lock(&lock), &pins))
}

/// A package in `pnpm-lock.yaml` (lockfile 9): name, version, SHA-512 in hex.
#[must_use]
pub fn parse_pnpm_lock(text: &str) -> Vec<(String, String, Option<String>)> {
    let mut out = Vec::new();
    let mut inside = false;
    for line in text.lines() {
        if !line.starts_with(' ') && !line.is_empty() {
            inside = line == "packages:";
            continue;
        }
        if !inside {
            continue;
        }
        if let Some(key) = line.strip_prefix("  ").filter(|l| !l.starts_with(' ')) {
            let key = key.trim_end_matches(':').trim_matches('\'');
            // `@scope/name@1.2.3` or `name@1.2.3`; the version is after the last `@`.
            if let Some((name, version)) = key.rsplit_once('@').filter(|(n, _)| !n.is_empty()) {
                out.push((name.to_owned(), version.to_owned(), None));
            }
        } else if let (Some(last), Some(i)) = (out.last_mut(), line.find("integrity: sha512-")) {
            let b64 = line
                .get(i + "integrity: sha512-".len()..)
                .unwrap_or_default()
                .trim_end_matches(['}', ' ', ','])
                .trim();
            last.2 = base64_to_hex(b64);
        }
    }
    out
}

fn base64_to_hex(s: &str) -> Option<String> {
    const ABC: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut bits: u32 = 0;
    let mut n = 0;
    let mut hex = String::new();
    for c in s.bytes().take_while(|&c| c != b'=') {
        let v = u32::try_from(ABC.iter().position(|&a| a == c)?).ok()?;
        bits = (bits << 6) | v;
        n += 6;
        if n >= 8 {
            n -= 8;
            hex.push_str(&format!("{:02x}", (bits >> n) & 0xff));
        }
    }
    Some(hex)
}

/// The CycloneDX document.
#[must_use]
pub fn sbom_json(cargo_lock: &str, pnpm_lock: &str, version: &str) -> serde_json::Value {
    use serde_json::json;
    let mut components: BTreeMap<String, serde_json::Value> = BTreeMap::new();
    for p in parse_cargo_lock(cargo_lock) {
        let Some(source) = &p.source else { continue };
        let purl = format!("pkg:cargo/{}@{}", p.name, p.version);
        let mut c = json!({
            "type": "library",
            "bom-ref": purl,
            "name": p.name,
            "version": p.version,
            "purl": purl,
            "properties": [{ "name": "cargo:source", "value": source }],
        });
        if let Some(sum) = &p.checksum {
            c["hashes"] = json!([{ "alg": "SHA-256", "content": sum }]);
        }
        components.insert(purl, c);
    }
    for (name, version, sha512) in parse_pnpm_lock(pnpm_lock) {
        let encoded = name.replacen('@', "%40", 1);
        let purl = format!("pkg:npm/{encoded}@{version}");
        let mut c = json!({
            "type": "library",
            "bom-ref": purl,
            "name": name,
            "version": version,
            "purl": purl,
        });
        if let Some(h) = sha512 {
            c["hashes"] = json!([{ "alg": "SHA-512", "content": h }]);
        }
        components.insert(purl, c);
    }
    json!({
        "bomFormat": "CycloneDX",
        "specVersion": "1.5",
        "version": 1,
        "metadata": {
            "component": {
                "type": "application",
                "bom-ref": "diagnostic-vault",
                "name": "diagnostic-vault",
                "version": version,
            },
            "tools": { "components": [{ "type": "application", "name": "cargo xtask sbom" }] },
        },
        "components": components.into_values().collect::<Vec<_>>(),
    })
}

fn workspace_version(root: &Path) -> String {
    fs::read_to_string(root.join("Cargo.toml"))
        .ok()
        .and_then(|t| {
            t.lines()
                .skip_while(|l| l.trim() != "[workspace.package]")
                .find_map(|l| l.trim().strip_prefix("version = ").map(unquote))
        })
        .unwrap_or_else(|| "0.0.0".to_owned())
}

/// Writes the SBOM to `out`; returns how many components it lists.
pub fn write_sbom(root: &Path, out: &Path) -> Result<usize, String> {
    let cargo =
        fs::read_to_string(root.join("Cargo.lock")).map_err(|e| format!("Cargo.lock: {e}"))?;
    let pnpm = fs::read_to_string(root.join("apps/desktop/pnpm-lock.yaml"))
        .map_err(|e| format!("pnpm-lock.yaml: {e}"))?;
    let doc = sbom_json(&cargo, &pnpm, &workspace_version(root));
    let n = doc["components"].as_array().map_or(0, Vec::len);
    let text = serde_json::to_string_pretty(&doc).map_err(|e| e.to_string())?;
    fs::write(out, text + "\n").map_err(|e| format!("{}: {e}", out.display()))?;
    Ok(n)
}

pub fn sbom(root: &Path, args: &[String]) -> Result<String, String> {
    let out = args.first().ok_or("usage: cargo xtask sbom <out.json>")?;
    let n = write_sbom(root, Path::new(out))?;
    Ok(format!("{out}: {n} components"))
}

/// The pinned update key file holds one or two valid keys.
fn key_problems(root: &Path) -> Vec<String> {
    let text = fs::read_to_string(root.join(PUBLIC_FILE)).unwrap_or_default();
    let keys = dv_egress::update::parse_keys(&text);
    if keys.is_empty() {
        vec![format!(
            "{PUBLIC_FILE}: no valid key (one or two lines of 64 hex digits)"
        )]
    } else {
        Vec::new()
    }
}

/// SEC8: invariants, the data scan, parser pins, the update key and the SBOM; `--full` adds
/// the privacy suite (0 leaks).
pub fn self_audit(root: &Path, args: &[String]) -> Vec<String> {
    let mut problems = Vec::new();
    let mut step = |title: &str, r: Result<Vec<String>, String>| {
        use std::io::Write;
        let _ = writeln!(std::io::stderr().lock(), "== {title}");
        match r {
            Ok(p) => problems.extend(p.into_iter().map(|p| format!("{title}: {p}"))),
            Err(e) => problems.push(format!("{title}: could not run: {e}")),
        }
    };
    step(
        "invariants",
        crate::invariants::check_all(root).map_err(|e| e.to_string()),
    );
    step(
        "scan",
        crate::scan::run(root, crate::scan::Mode::All).map_err(|e| e.to_string()),
    );
    step("parser pins", check_parsers(root));
    step("update key", Ok(key_problems(root)));
    let out = std::env::temp_dir().join("dv-self-audit-sbom.json");
    step("sbom", write_sbom(root, &out).map(|_| Vec::new()));
    let _ = fs::remove_file(&out);
    if args.iter().any(|a| a == "--full") {
        let r = Command::new("cargo")
            .args(["test", "-p", "dv-privacy"])
            .current_dir(root)
            .status()
            .map_err(|e| e.to_string())
            .map(|s| {
                if s.success() {
                    Vec::new()
                } else {
                    vec![format!("failed ({s})")]
                }
            });
        step("privacy suite", r);
    }
    problems
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    const LOCK: &str = r#"version = 4

[[package]]
name = "dv-core"
version = "0.1.0"
dependencies = [
 "lopdf",
]

[[package]]
name = "lopdf"
version = "0.42.0"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "aaaa"

[[package]]
name = "png"
version = "0.18.1"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "bbbb"
"#;

    #[test]
    fn a_parser_version_that_is_not_pinned_fails_and_a_changed_checksum_fails_too() {
        let lock = parse_cargo_lock(LOCK);
        assert_eq!(lock.len(), 3);
        let pins = "# watch: lopdf png\nlopdf 0.42.0 aaaa\npng 0.18.1 bbbb\n";
        assert!(parser_problems(&lock, pins).is_empty());
        let missing = parser_problems(&lock, "# watch: lopdf png\nlopdf 0.42.0 aaaa\n");
        assert_eq!(missing.len(), 1);
        assert!(missing[0].starts_with("png 0.18.1"));
        let changed = parser_problems(
            &lock,
            "# watch: lopdf png\nlopdf 0.42.0 cccc\npng 0.18.1 bbbb\n",
        );
        assert_eq!(changed.len(), 1, "same version, other contents");
        assert_eq!(
            parser_problems(&lock, "lopdf 0.42.0 aaaa\n").len(),
            1,
            "no watch line"
        );
    }

    #[test]
    fn the_sbom_lists_crates_and_npm_packages_with_their_checksums() {
        let pnpm = "lockfileVersion: '9.0'\n\npackages:\n\n  '@scope/a@1.0.0':\n    resolution: {integrity: sha512-AAEC}\n\n  b@2.0.0:\n    resolution: {integrity: sha512-/w==}\n\nsnapshots:\n\n  b@2.0.0: {}\n";
        let doc = sbom_json(LOCK, pnpm, "0.3.0");
        let comps = doc["components"].as_array().unwrap();
        let purls: Vec<&str> = comps.iter().map(|c| c["purl"].as_str().unwrap()).collect();
        assert_eq!(
            purls,
            [
                "pkg:cargo/lopdf@0.42.0",
                "pkg:cargo/png@0.18.1",
                "pkg:npm/%40scope/a@1.0.0",
                "pkg:npm/b@2.0.0"
            ],
            "the workspace's own crates and the snapshots section are not components"
        );
        assert_eq!(comps[0]["hashes"][0]["content"], "aaaa");
        assert_eq!(comps[2]["hashes"][0]["content"], "000102");
        assert_eq!(comps[3]["hashes"][0]["content"], "ff");
        assert_eq!(doc["metadata"]["component"]["version"], "0.3.0");
        assert!(
            doc["metadata"].get("timestamp").is_none(),
            "same tree, same file"
        );
    }

    #[test]
    fn the_repository_passes_its_own_parser_pins() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        assert_eq!(check_parsers(root).unwrap(), Vec::<String>::new());
    }
}
