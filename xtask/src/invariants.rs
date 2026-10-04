//! Architecture invariants (docs/ARCHITECTURE.md → "אינווריאנטים שנאכפים ב-CI").

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use cargo_metadata::{DependencyKind, MetadataCommand, PackageId};
use regex::Regex;

/// The only workspace crate allowed to reach the network.
pub const EGRESS_CRATE: &str = "dv-egress";

/// Crates that open network connections, speak TLS, or phone home.
pub const BANNED_OUTSIDE_EGRESS: &[&str] = &[
    "reqwest",
    "hyper",
    "hyper-util",
    "ureq",
    "isahc",
    "curl",
    "surf",
    "attohttpc",
    "h2",
    "h3",
    "quinn",
    "rustls",
    "native-tls",
    "tokio-rustls",
    "tokio-native-tls",
    "tungstenite",
    "tokio-tungstenite",
    "sentry",
    "opentelemetry",
];

/// Tauri plugins that would hand the WebView file, shell or network access.
pub const BANNED_TAURI_PLUGINS: &[&str] = &[
    "tauri-plugin-http",
    "tauri-plugin-fs",
    "tauri-plugin-shell",
    "tauri-plugin-updater",
    "tauri-plugin-websocket",
    "tauri-plugin-upload",
    "tauri-plugin-sql",
];

pub fn check_all(root: &Path) -> Result<Vec<String>, String> {
    let mut problems = network_isolation(root)?;
    problems.extend(unsafe_policy(root)?);
    problems.extend(ui_rules(&root.join("apps/desktop/src")));
    problems.extend(tauri_hardening(&root.join("apps/desktop/src-tauri")));
    problems.extend(cleared_payload_construction(&root.join("crates")));
    Ok(problems)
}

/// A dependency graph reduced to what the network check needs.
#[derive(Debug, Default)]
pub struct Graph {
    pub names: BTreeMap<String, String>,
    pub edges: BTreeMap<String, Vec<String>>,
}

/// Every path from `start` to a banned crate that does not pass through the egress crate.
pub fn banned_paths(graph: &Graph, start: &str, banned: &[&str]) -> Vec<String> {
    let mut found = Vec::new();
    let mut seen = BTreeSet::new();
    let mut stack = vec![(start.to_owned(), vec![start.to_owned()])];
    while let Some((node, path)) = stack.pop() {
        if !seen.insert(node.clone()) {
            continue;
        }
        for next in graph.edges.get(&node).into_iter().flatten() {
            let name = graph.names.get(next).map_or(next.as_str(), String::as_str);
            if name == EGRESS_CRATE {
                continue;
            }
            let mut next_path = path.clone();
            next_path.push(next.clone());
            if banned.contains(&name) {
                let rendered: Vec<&str> = next_path
                    .iter()
                    .map(|id| graph.names.get(id).map_or(id.as_str(), String::as_str))
                    .collect();
                found.push(rendered.join(" → "));
            } else {
                stack.push((next.clone(), next_path));
            }
        }
    }
    found
}

/// Platforms we ship to. Dependencies that exist only for other targets (e.g. Tauri's
/// mobile-only HTTP client) are not compiled into our binaries.
pub const SHIPPED_TARGETS: &[&str] = &[
    "x86_64-pc-windows-msvc",
    "aarch64-pc-windows-msvc",
    "x86_64-apple-darwin",
    "aarch64-apple-darwin",
    "x86_64-unknown-linux-gnu",
];

fn network_isolation(root: &Path) -> Result<Vec<String>, String> {
    let mut problems = BTreeSet::new();
    for target in SHIPPED_TARGETS {
        for p in network_isolation_for(root, target)? {
            problems.insert(format!("[{target}] {p}"));
        }
    }
    Ok(problems.into_iter().collect())
}

fn network_isolation_for(root: &Path, target: &str) -> Result<Vec<String>, String> {
    let metadata = MetadataCommand::new()
        .manifest_path(root.join("Cargo.toml"))
        .other_options(vec!["--filter-platform".to_owned(), target.to_owned()])
        .exec()
        .map_err(|e| e.to_string())?;
    let resolve = metadata
        .resolve
        .as_ref()
        .ok_or("cargo metadata returned no resolve graph")?;

    let mut graph = Graph::default();
    for package in &metadata.packages {
        graph
            .names
            .insert(package.id.repr.clone(), package.name.to_string());
    }
    for node in &resolve.nodes {
        let targets: Vec<String> = node
            .deps
            .iter()
            .filter(|d| d.dep_kinds.iter().any(|k| k.kind == DependencyKind::Normal))
            .map(|d| d.pkg.repr.clone())
            .collect();
        graph.edges.insert(node.id.repr.clone(), targets);
    }

    let mut problems = Vec::new();
    let members: Vec<&PackageId> = metadata.workspace_members.iter().collect();
    for id in members {
        let name = graph.names.get(&id.repr).cloned().unwrap_or_default();
        if name == EGRESS_CRATE || name == "xtask" {
            continue;
        }
        for path in banned_paths(&graph, &id.repr, BANNED_OUTSIDE_EGRESS) {
            problems.push(format!("network crate outside {EGRESS_CRATE}: {path}"));
        }
        for path in banned_paths(&graph, &id.repr, BANNED_TAURI_PLUGINS) {
            problems.push(format!("forbidden Tauri plugin: {path}"));
        }
    }
    Ok(problems)
}

fn unsafe_policy(root: &Path) -> Result<Vec<String>, String> {
    let workspace = fs::read_to_string(root.join("Cargo.toml")).map_err(|e| e.to_string())?;
    let mut problems = Vec::new();
    if !workspace.contains("unsafe_code = \"forbid\"") {
        problems.push("workspace lints must set unsafe_code = \"forbid\"".to_owned());
    }
    let mut manifests = vec![root.join("xtask/Cargo.toml")];
    if let Ok(entries) = fs::read_dir(root.join("crates")) {
        manifests.extend(entries.flatten().map(|e| e.path().join("Cargo.toml")));
    }
    for manifest in manifests {
        let text = fs::read_to_string(&manifest).map_err(|e| e.to_string())?;
        // The one audited exception (D-044): `deny`, opted into block by block in windows.rs.
        if manifest.ends_with("crates/dv-sandbox/Cargo.toml") {
            if !text.contains("unsafe_code = \"deny\"") {
                problems.push("dv-sandbox must set unsafe_code = \"deny\"".to_owned());
            }
            continue;
        }
        if !text.contains("[lints]\nworkspace = true") {
            problems.push(format!(
                "{} must inherit workspace lints",
                manifest.display()
            ));
        }
    }
    let shell = fs::read_to_string(root.join("apps/desktop/src-tauri/Cargo.toml"))
        .map_err(|e| e.to_string())?;
    if !shell.contains("unsafe_code = \"deny\"") {
        problems.push("desktop shell must set unsafe_code = \"deny\"".to_owned());
    }
    Ok(problems)
}

/// Patterns that would render HTML or execute strings in the WebView.
pub fn ui_violations(source: &str) -> Vec<&'static str> {
    const RULES: &[(&str, &str)] = &[
        (r"dangerouslySetInnerHTML", "dangerouslySetInnerHTML"),
        (r"\.(inner|outer)HTML\s*=", "innerHTML/outerHTML assignment"),
        (r"insertAdjacentHTML", "insertAdjacentHTML"),
        (r"\beval\s*\(", "eval()"),
        (r"new\s+Function\s*\(", "new Function()"),
        (
            r"(fetch|XMLHttpRequest|WebSocket|EventSource)\s*\(",
            "network API in the UI",
        ),
    ];
    RULES
        .iter()
        .filter(|(pattern, _)| Regex::new(pattern).is_ok_and(|re| re.is_match(source)))
        .map(|(_, label)| *label)
        .collect()
}

fn ui_rules(src: &Path) -> Vec<String> {
    let mut problems = Vec::new();
    let mut stack = vec![src.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let is_source = path.extension().is_some_and(|e| e == "ts" || e == "tsx");
            let is_test = path.to_string_lossy().contains(".test.");
            if !is_source || is_test {
                continue;
            }
            let text = fs::read_to_string(&path).unwrap_or_default();
            for label in ui_violations(&text) {
                problems.push(format!("{}: {label}", path.display()));
            }
        }
    }
    problems
}

/// Checks on `tauri.conf.json` and the capability files.
pub fn tauri_config_violations(
    conf: &serde_json::Value,
    capabilities: &[serde_json::Value],
) -> Vec<String> {
    let mut problems = Vec::new();
    let security = &conf["app"]["security"];
    let csp = &security["csp"];
    if csp["connect-src"] != "ipc: http://ipc.localhost" {
        problems.push("CSP connect-src must be exactly `ipc: http://ipc.localhost`".to_owned());
    }
    for (directive, expected) in [
        ("default-src", "'self'"),
        ("script-src", "'self'"),
        ("object-src", "'none'"),
        ("frame-ancestors", "'none'"),
    ] {
        if csp[directive] != expected {
            problems.push(format!("CSP {directive} must be {expected}"));
        }
    }
    if security["freezePrototype"] != true {
        problems.push("security.freezePrototype must be true".to_owned());
    }
    if security
        .get("dangerousDisableAssetCspModification")
        .is_some()
    {
        problems.push("dangerousDisableAssetCspModification must not be set".to_owned());
    }
    if conf["app"]["withGlobalTauri"] != false {
        problems.push("app.withGlobalTauri must be false".to_owned());
    }
    let windows = conf["app"]["windows"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    if windows.is_empty() || windows.iter().any(|w| w["contentProtected"] != true) {
        problems.push("every window must set contentProtected: true".to_owned());
    }
    if conf
        .get("plugins")
        .is_some_and(|p| p.as_object().is_some_and(|o| !o.is_empty()))
    {
        problems.push("no Tauri plugins may be configured".to_owned());
    }
    for cap in capabilities {
        if cap.get("remote").is_some() {
            problems.push("capabilities must not grant remote URLs".to_owned());
        }
        for permission in cap["permissions"].as_array().into_iter().flatten() {
            let id = permission
                .as_str()
                .or_else(|| permission["identifier"].as_str())
                .unwrap_or("");
            if !id.starts_with("allow-") {
                problems.push(format!("capability grants non-app permission `{id}`"));
            }
        }
    }
    problems
}

fn tauri_hardening(shell: &Path) -> Vec<String> {
    let read_json = |p: &Path| -> Result<serde_json::Value, String> {
        let text = fs::read_to_string(p).map_err(|e| format!("{}: {e}", p.display()))?;
        serde_json::from_str(&text).map_err(|e| format!("{}: {e}", p.display()))
    };
    let conf = match read_json(&shell.join("tauri.conf.json")) {
        Ok(c) => c,
        Err(e) => return vec![e],
    };
    let mut capabilities = Vec::new();
    if let Ok(entries) = fs::read_dir(shell.join("capabilities")) {
        for entry in entries.flatten() {
            match read_json(&entry.path()) {
                Ok(c) => capabilities.push(c),
                Err(e) => return vec![e],
            }
        }
    }
    tauri_config_violations(&conf, &capabilities)
}

/// `ClearedPayload { .. }` may only be constructed inside the gate.
fn cleared_payload_construction(crates: &Path) -> Vec<String> {
    let Ok(re) = Regex::new(r"ClearedPayload\s*\{") else {
        return Vec::new();
    };
    let allowed = crates.join("dv-privacy/src/gate.rs");
    let mut problems = Vec::new();
    let mut stack = vec![crates.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "rs") && path != allowed {
                let text = fs::read_to_string(&path).unwrap_or_default();
                let constructs = text
                    .lines()
                    .any(|l| re.is_match(l) && !l.contains("struct ClearedPayload"));
                if constructs {
                    problems.push(format!(
                        "{}: constructs ClearedPayload outside the gate",
                        path.display()
                    ));
                }
            }
        }
    }
    problems
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn graph(edges: &[(&str, &str)]) -> Graph {
        let mut g = Graph::default();
        for (a, b) in edges {
            g.names.insert((*a).to_owned(), (*a).to_owned());
            g.names.insert((*b).to_owned(), (*b).to_owned());
            g.edges
                .entry((*a).to_owned())
                .or_default()
                .push((*b).to_owned());
        }
        g
    }

    #[test]
    fn network_crate_reached_directly_is_reported() {
        let g = graph(&[("dv-vault", "reqwest")]);
        assert_eq!(
            banned_paths(&g, "dv-vault", BANNED_OUTSIDE_EGRESS),
            vec!["dv-vault → reqwest"]
        );
    }

    #[test]
    fn network_crate_reached_transitively_is_reported() {
        let g = graph(&[("dv-privacy", "helper"), ("helper", "hyper")]);
        assert_eq!(
            banned_paths(&g, "dv-privacy", BANNED_OUTSIDE_EGRESS).len(),
            1
        );
    }

    #[test]
    fn network_crate_behind_egress_is_allowed() {
        let g = graph(&[("dv-core", "dv-egress"), ("dv-egress", "reqwest")]);
        assert!(banned_paths(&g, "dv-core", BANNED_OUTSIDE_EGRESS).is_empty());
    }

    #[test]
    fn ui_rules_catch_html_injection_and_network_calls() {
        assert!(
            ui_violations("<div dangerouslySetInnerHTML={{__html: x}} />")
                .contains(&"dangerouslySetInnerHTML")
        );
        assert!(!ui_violations("el.innerHTML = s").is_empty());
        assert!(!ui_violations("await fetch(url)").is_empty());
        assert!(ui_violations("const text = <p>{reply}</p>;").is_empty());
    }

    fn good_conf() -> serde_json::Value {
        json!({
            "app": {
                "withGlobalTauri": false,
                "windows": [{"label": "main", "contentProtected": true}],
                "security": {
                    "freezePrototype": true,
                    "csp": {
                        "default-src": "'self'", "script-src": "'self'", "object-src": "'none'",
                        "frame-ancestors": "'none'", "connect-src": "ipc: http://ipc.localhost"
                    }
                }
            }
        })
    }

    #[test]
    fn hardened_config_passes() {
        let caps = vec![json!({"permissions": ["allow-ping"]})];
        assert!(tauri_config_violations(&good_conf(), &caps).is_empty());
    }

    #[test]
    fn weakened_config_fails() {
        let mut conf = good_conf();
        conf["app"]["security"]["csp"]["connect-src"] = json!("ipc: http://ipc.localhost https:");
        conf["app"]["windows"][0]["contentProtected"] = json!(false);
        let caps = vec![json!({"permissions": ["fs:default", "allow-ping"]})];
        let problems = tauri_config_violations(&conf, &caps);
        assert_eq!(problems.len(), 3, "{problems:?}");
    }
}
