//! The TypeScript side of every command, generated from the shell's `#[tauri::command]`
//! functions (F5). `ts-rs` already generates the types; this adds what each command takes and
//! answers, so `ipc/client.ts` is checked by `tsc` against the Rust signatures: a renamed
//! argument, a missing one or a changed answer is a compile error instead of a button that
//! works in the browser preview and fails in the app.
//!
//! `cargo xtask gen-ipc` writes `apps/desktop/src/ipc/generated/commands.ts`;
//! `check-invariants` fails while the checked-in copy is stale.

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use syn::{FnArg, GenericArgument, Item, Pat, PathArguments, ReturnType, Type};

const OUT: &str = "apps/desktop/src/ipc/generated/commands.ts";

/// Parameters Tauri fills in itself; the window never sends them.
const INJECTED: &[&str] = &["State", "AppHandle", "Window", "WebviewWindow", "Webview"];

#[derive(Debug)]
struct Command {
    name: String,
    /// `None`: the raw request body (bytes, with headers).
    args: Option<Vec<(String, String)>>,
    returns: String,
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            rust_files(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

fn is_command(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|a| {
        let segs: Vec<String> = a
            .path()
            .segments
            .iter()
            .map(|s| s.ident.to_string())
            .collect();
        segs == ["tauri", "command"]
    })
}

fn is_test_mod(m: &syn::ItemMod) -> bool {
    m.attrs.iter().any(|a| {
        a.path().is_ident("cfg") && a.parse_args::<syn::Ident>().is_ok_and(|i| i == "test")
    })
}

fn camel(snake: &str) -> String {
    let mut out = String::with_capacity(snake.len());
    let mut up = false;
    for c in snake.chars() {
        if c == '_' {
            up = true;
        } else if up {
            out.extend(c.to_uppercase());
            up = false;
        } else {
            out.push(c);
        }
    }
    out
}

fn last_segment(t: &Type) -> Option<&syn::PathSegment> {
    match t {
        Type::Path(p) => p.path.segments.last(),
        Type::Reference(r) => last_segment(&r.elem),
        _ => None,
    }
}

fn generics(seg: &syn::PathSegment) -> Vec<&Type> {
    match &seg.arguments {
        PathArguments::AngleBracketed(a) => a
            .args
            .iter()
            .filter_map(|g| match g {
                GenericArgument::Type(t) => Some(t),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// The TypeScript for a Rust type as serde and Tauri send it. A named type must be one
/// `ts-rs` exports; it goes into `imports`.
fn ts(t: &Type, imports: &mut BTreeSet<String>) -> Result<String, String> {
    match t {
        Type::Tuple(tuple) if tuple.elems.is_empty() => Ok("null".to_owned()),
        Type::Tuple(tuple) => {
            let parts: Result<Vec<String>, String> =
                tuple.elems.iter().map(|e| ts(e, imports)).collect();
            Ok(format!("[{}]", parts?.join(", ")))
        }
        Type::Reference(r) => ts(&r.elem, imports),
        Type::Path(_) => {
            let seg = last_segment(t).ok_or("an empty type path")?;
            let name = seg.ident.to_string();
            let args = generics(seg);
            let one = |imports: &mut BTreeSet<String>| -> Result<String, String> {
                let first = args
                    .first()
                    .ok_or_else(|| format!("{name} without its type"))?;
                ts(first, imports)
            };
            Ok(match name.as_str() {
                "String" | "str" | "char" => "string".to_owned(),
                "bool" => "boolean".to_owned(),
                "u8" | "u16" | "u32" | "u64" | "usize" | "i8" | "i16" | "i32" | "i64" | "isize"
                | "f32" | "f64" => "number".to_owned(),
                "Option" => format!("{} | null", one(imports)?),
                "Vec" => format!("Array<{}>", one(imports)?),
                "HashMap" | "BTreeMap" => {
                    let value = args.get(1).ok_or("a map without its value type")?;
                    format!("Record<string, {}>", ts(value, imports)?)
                }
                "Value" => "unknown".to_owned(),
                _ => {
                    imports.insert(name.clone());
                    name
                }
            })
        }
        _ => Err("a type the generator does not know".to_owned()),
    }
}

fn command(f: &syn::ItemFn, imports: &mut BTreeSet<String>) -> Result<Command, String> {
    let name = f.sig.ident.to_string();
    let mut args = Some(Vec::new());
    for input in &f.sig.inputs {
        let FnArg::Typed(pt) = input else {
            return Err(format!("{name}: a command cannot take self"));
        };
        let seg = last_segment(&pt.ty)
            .map(|s| s.ident.to_string())
            .unwrap_or_default();
        if INJECTED.contains(&seg.as_str()) {
            continue;
        }
        if seg == "Request" {
            args = None;
            continue;
        }
        let Pat::Ident(id) = &*pt.pat else {
            return Err(format!("{name}: an argument that is not a plain name"));
        };
        let ty = ts(&pt.ty, imports).map_err(|e| format!("{name}: {e}"))?;
        if let Some(a) = args.as_mut() {
            a.push((camel(&id.ident.to_string()), ty));
        }
    }
    let returns = match &f.sig.output {
        ReturnType::Default => "null".to_owned(),
        ReturnType::Type(_, t) => {
            let inner = match last_segment(t) {
                Some(seg) if seg.ident == "Res" || seg.ident == "Result" => generics(seg)
                    .first()
                    .copied()
                    .ok_or_else(|| format!("{name}: a Result without its type"))?,
                _ => &**t,
            };
            ts(inner, imports).map_err(|e| format!("{name}: {e}"))?
        }
    };
    Ok(Command {
        name,
        args,
        returns,
    })
}

fn collect(
    items: &[Item],
    imports: &mut BTreeSet<String>,
    out: &mut Vec<Command>,
) -> Result<(), String> {
    for item in items {
        match item {
            Item::Fn(f) if is_command(&f.attrs) => out.push(command(f, imports)?),
            Item::Mod(m) if !is_test_mod(m) => {
                if let Some((_, inner)) = &m.content {
                    collect(inner, imports, out)?;
                }
            }
            _ => {}
        }
    }
    Ok(())
}

/// The generated file's text, from the shell's sources.
pub fn render(root: &Path) -> Result<String, String> {
    let mut files = Vec::new();
    rust_files(&root.join("apps/desktop/src-tauri/src"), &mut files);
    files.sort();
    let mut imports = BTreeSet::new();
    let mut commands = Vec::new();
    for path in &files {
        let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let file = syn::parse_file(&text).map_err(|e| format!("{}: {e}", path.display()))?;
        collect(&file.items, &mut imports, &mut commands)?;
    }
    if commands.is_empty() {
        return Err("no #[tauri::command] functions found".to_owned());
    }
    commands.sort_by(|a, b| a.name.cmp(&b.name));
    let generated = root.join("apps/desktop/src/ipc/generated");
    let missing: Vec<&String> = imports
        .iter()
        .filter(|n| !generated.join(format!("{n}.ts")).is_file())
        .collect();
    if !missing.is_empty() {
        return Err(format!(
            "commands use types that ts-rs does not export (add #[ts(export)]): {missing:?}"
        ));
    }
    let mut s = String::from(
        "// Generated by `cargo xtask gen-ipc` from the shell's #[tauri::command] functions. \
         Do not edit.\n",
    );
    for n in &imports {
        let _ = writeln!(s, "import type {{ {n} }} from \"./{n}\";");
    }
    s.push_str(
        "\n/** Every command: the arguments `invoke` sends (camelCase, as Tauri reads them) and the\n \
         * answer. `args: undefined` takes nothing; `Uint8Array` is the raw request body. */\n\
         export type Commands = {\n",
    );
    for c in &commands {
        let args = match &c.args {
            None => "Uint8Array".to_owned(),
            Some(a) if a.is_empty() => "undefined".to_owned(),
            Some(a) => {
                let fields: Vec<String> = a.iter().map(|(n, t)| format!("{n}: {t}")).collect();
                format!("{{ {} }}", fields.join("; "))
            }
        };
        let _ = writeln!(
            s,
            "  {}: {{ args: {args}; returns: {} }};",
            c.name, c.returns
        );
    }
    s.push_str("};\n");
    Ok(s)
}

pub fn write(root: &Path) -> Result<String, String> {
    let text = render(root)?;
    fs::write(root.join(OUT), &text).map_err(|e| format!("{OUT}: {e}"))?;
    Ok(format!("wrote {OUT}"))
}

/// For `check-invariants`: the checked-in file matches the shell's commands.
pub fn stale(root: &Path) -> Vec<String> {
    match render(root) {
        Ok(text) if fs::read_to_string(root.join(OUT)).is_ok_and(|now| now == text) => Vec::new(),
        Ok(_) => vec![format!("{OUT} is stale: run `cargo xtask gen-ipc`")],
        Err(e) => vec![format!("gen-ipc: {e}")],
    }
}
