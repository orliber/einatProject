//! The local name model, downloaded only when she turns it on in Settings (D-053).
//!
//! What leaves: three `GET` requests to Hugging Face for public files, with a fixed user
//! agent and nothing from the vault. What is trusted: only files whose size and SHA-256 are
//! pinned below, from one fixed revision of the model. Anything else (another host, a
//! different size or hash, a cut download) leaves nothing on disk, and the filter keeps
//! working on its rules alone.

use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use aws_lc_rs::digest;

/// `dicta-il/dictabert-ner` (CC BY 4.0), pinned to one commit so the files never change under us.
pub const MODEL_REPO: &str = "dicta-il/dictabert-ner";
pub const MODEL_REVISION: &str = "05ed61bc99437b13bc2bdef8154c52f8b44eaad6";
/// The folder name under the models folder; a new revision gets a new folder.
pub const MODEL_FOLDER: &str = "dictabert-ner-05ed61bc";

/// One pinned file: name, size in bytes, SHA-256 (hex).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModelFile {
    pub name: &'static str,
    pub size: u64,
    pub sha256: &'static str,
}

pub const MODEL_FILES: &[ModelFile] = &[
    ModelFile {
        name: "config.json",
        size: 1_673,
        sha256: "c33e5cb2a330032280ebeb92e8a603e161926c96e23ac4651673ddc3564bd443",
    },
    ModelFile {
        name: "tokenizer.json",
        size: 3_585_660,
        sha256: "640e96366c04454301714d5aadce9310c9f03275e6d50b7329e7c93a72d18c17",
    },
    ModelFile {
        name: "model.safetensors",
        size: 735_125_420,
        sha256: "beeea5e7bfb6a8acf6765abc22400b27d0ae88afb177a89009574ddc0c1f4188",
    },
];

/// Hugging Face answers a file with a redirect to its own file storage (`*.hf.co`).
const MODEL_HOST: &str = "huggingface.co";
const MODEL_CDN_SUFFIX: &str = ".hf.co";
const PART: &str = ".part";

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ModelError {
    #[error("no connection to the model server")]
    Offline,
    #[error("the model server answered {0}")]
    Server(u16),
    #[error("the downloaded file does not match the pinned one: {0}")]
    Tampered(String),
    #[error("the address is not on the model list: {0}")]
    HostRefused(String),
    #[error("the model folder: {0}")]
    Disk(String),
}

/// Whatever streams a file: Hugging Face in the program, a table in the tests.
pub trait ModelFetch {
    /// Write the body of `url` to `out`, at most `max` bytes, calling `progress` with the
    /// bytes written so far. Returns how many were written.
    fn get_to(
        &self,
        url: &str,
        out: &mut dyn Write,
        max: u64,
        progress: &dyn Fn(u64),
    ) -> Result<u64, ModelError>;
}

/// The address of one pinned file.
#[must_use]
pub fn file_url(name: &str) -> String {
    format!("https://{MODEL_HOST}/{MODEL_REPO}/resolve/{MODEL_REVISION}/{name}")
}

/// Total bytes of the pinned files.
#[must_use]
pub fn total_size() -> u64 {
    MODEL_FILES.iter().map(|f| f.size).sum()
}

/// Is every pinned file in `dir`, with its size? (Cheap: the hashes are checked by [`verify`].)
#[must_use]
pub fn present(dir: &Path) -> bool {
    MODEL_FILES
        .iter()
        .all(|f| fs::metadata(dir.join(f.name)).is_ok_and(|m| m.len() == f.size))
}

/// Every pinned file in `dir` has its pinned hash.
pub fn verify(dir: &Path) -> Result<(), ModelError> {
    for f in MODEL_FILES {
        let mut file = File::open(dir.join(f.name))
            .map_err(|e| ModelError::Disk(format!("{}: {e}", f.name)))?;
        let (size, hash) = hash_reader(&mut file).map_err(|e| ModelError::Disk(e.to_string()))?;
        if size != f.size || hash != f.sha256 {
            return Err(ModelError::Tampered(f.name.to_owned()));
        }
    }
    Ok(())
}

fn hash_reader(r: &mut dyn Read) -> std::io::Result<(u64, String)> {
    let mut ctx = digest::Context::new(&digest::SHA256);
    let mut buf = vec![0u8; 1 << 20];
    let mut size = 0u64;
    loop {
        let n = r.read(&mut buf)?;
        if n == 0 {
            break;
        }
        ctx.update(buf.get(..n).unwrap_or_default());
        size += n as u64;
    }
    Ok((size, crate::update::to_hex(ctx.finish().as_ref())))
}

/// A writer that hashes what passes through it.
struct Hashing<'a> {
    inner: &'a mut dyn Write,
    ctx: digest::Context,
}

impl Write for Hashing<'_> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let n = self.inner.write(buf)?;
        self.ctx.update(buf.get(..n).unwrap_or_default());
        Ok(n)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

/// Download the pinned files into `dir`, each checked against its size and hash before it
/// takes its name. `progress` gets the bytes done of [`total_size`]. A file already there
/// with the right hash is not fetched again. On any failure nothing half-written stays.
pub fn download(
    fetch: &dyn ModelFetch,
    dir: &Path,
    progress: &dyn Fn(u64),
) -> Result<(), ModelError> {
    fs::create_dir_all(dir).map_err(|e| ModelError::Disk(e.to_string()))?;
    let mut done = 0u64;
    for f in MODEL_FILES {
        let path = dir.join(f.name);
        let intact = File::open(&path)
            .ok()
            .and_then(|mut file| hash_reader(&mut file).ok())
            .is_some_and(|(size, hash)| size == f.size && hash == f.sha256);
        if !intact {
            let part = part_path(&path);
            let result = fetch_one(fetch, f, &part, &|n| progress(done + n));
            if let Err(e) = result {
                let _ = fs::remove_file(&part);
                return Err(e);
            }
            fs::rename(&part, &path).map_err(|e| ModelError::Disk(e.to_string()))?;
        }
        done += f.size;
        progress(done);
    }
    Ok(())
}

fn part_path(path: &Path) -> PathBuf {
    let mut p = path.as_os_str().to_owned();
    p.push(PART);
    PathBuf::from(p)
}

fn fetch_one(
    fetch: &dyn ModelFetch,
    f: &ModelFile,
    part: &Path,
    progress: &dyn Fn(u64),
) -> Result<(), ModelError> {
    let mut file = File::create(part).map_err(|e| ModelError::Disk(e.to_string()))?;
    let mut hashing = Hashing {
        inner: &mut file,
        ctx: digest::Context::new(&digest::SHA256),
    };
    let size = fetch.get_to(&file_url(f.name), &mut hashing, f.size, progress)?;
    let hash = crate::update::to_hex(hashing.ctx.finish().as_ref());
    file.sync_all()
        .map_err(|e| ModelError::Disk(e.to_string()))?;
    if size != f.size || hash != f.sha256 {
        return Err(ModelError::Tampered(f.name.to_owned()));
    }
    Ok(())
}

/// Remove the model folder (turned off in Settings).
pub fn remove(dir: &Path) -> Result<(), ModelError> {
    match fs::remove_dir_all(dir) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(ModelError::Disk(e.to_string())),
    }
}

fn host_allowed(url: &reqwest::Url) -> bool {
    url.scheme() == "https"
        && url
            .host_str()
            .is_some_and(|h| h == MODEL_HOST || h.ends_with(MODEL_CDN_SUFFIX))
}

/// Hugging Face over TLS 1.3 with Mozilla's roots; redirects only to its own hosts.
#[derive(Debug)]
pub struct HuggingFaceFetch {
    client: reqwest::blocking::Client,
}

impl HuggingFaceFetch {
    pub fn new() -> Result<Self, ModelError> {
        let policy = reqwest::redirect::Policy::custom(|attempt| {
            if attempt.previous().len() >= 5 || !host_allowed(attempt.url()) {
                attempt.stop()
            } else {
                attempt.follow()
            }
        });
        let client = reqwest::blocking::Client::builder()
            .use_preconfigured_tls(crate::tls_config())
            .https_only(true)
            .no_proxy()
            .redirect(policy)
            .timeout(Duration::from_secs(3600))
            .connect_timeout(Duration::from_secs(20))
            .user_agent("diagnostic-vault-model")
            .build()
            .map_err(|_| ModelError::Offline)?;
        Ok(Self { client })
    }
}

impl ModelFetch for HuggingFaceFetch {
    fn get_to(
        &self,
        url: &str,
        out: &mut dyn Write,
        max: u64,
        progress: &dyn Fn(u64),
    ) -> Result<u64, ModelError> {
        let parsed =
            reqwest::Url::parse(url).map_err(|_| ModelError::HostRefused(url.to_owned()))?;
        if !host_allowed(&parsed) {
            return Err(ModelError::HostRefused(url.to_owned()));
        }
        let resp = self
            .client
            .get(parsed)
            .send()
            .map_err(|_| ModelError::Offline)?;
        if !host_allowed(resp.url()) {
            return Err(ModelError::HostRefused(resp.url().to_string()));
        }
        let status = resp.status().as_u16();
        if status != 200 {
            return Err(ModelError::Server(status));
        }
        copy_limited(resp, out, max, progress)
    }
}

/// Copy at most `max` bytes; one more means the file is not the pinned one.
fn copy_limited(
    from: impl Read,
    out: &mut dyn Write,
    max: u64,
    progress: &dyn Fn(u64),
) -> Result<u64, ModelError> {
    let mut from = from.take(max + 1);
    let mut buf = vec![0u8; 1 << 20];
    let mut n = 0u64;
    loop {
        let got = from.read(&mut buf).map_err(|_| ModelError::Offline)?;
        if got == 0 {
            break;
        }
        n += got as u64;
        if n > max {
            return Err(ModelError::Tampered(
                "larger than the pinned file".to_owned(),
            ));
        }
        out.write_all(buf.get(..got).unwrap_or_default())
            .map_err(|e| ModelError::Disk(e.to_string()))?;
        progress(n);
    }
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::collections::HashMap;

    /// Serves the given bodies by URL, recording what was asked.
    struct Table {
        files: HashMap<String, Vec<u8>>,
        asked: std::cell::RefCell<Vec<String>>,
    }
    impl ModelFetch for Table {
        fn get_to(
            &self,
            url: &str,
            out: &mut dyn Write,
            max: u64,
            progress: &dyn Fn(u64),
        ) -> Result<u64, ModelError> {
            self.asked.borrow_mut().push(url.to_owned());
            let b = self.files.get(url).ok_or(ModelError::Server(404))?;
            copy_limited(b.as_slice(), out, max, progress)
        }
    }

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("dv-model-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn the_pinned_list_is_one_revision_on_huggingface() {
        for f in MODEL_FILES {
            let url = file_url(f.name);
            assert!(url.starts_with("https://huggingface.co/dicta-il/dictabert-ner/resolve/"));
            assert!(url.contains(MODEL_REVISION));
            assert_eq!(f.sha256.len(), 64);
        }
        assert_eq!(total_size(), 738_712_753);
    }

    #[test]
    fn only_huggingface_hosts_over_https() {
        let ok = |u: &str| reqwest::Url::parse(u).is_ok_and(|u| host_allowed(&u));
        assert!(ok(
            "https://huggingface.co/dicta-il/dictabert-ner/resolve/x/config.json"
        ));
        assert!(ok("https://us.aws.cdn.hf.co/xet-bridge-us/abc"));
        assert!(ok("https://cas-bridge.xethub.hf.co/abc"));
        assert!(!ok("http://huggingface.co/x"));
        assert!(!ok("https://huggingface.co.evil.example/x"));
        assert!(!ok("https://evilhf.co/x"));
        assert!(!ok("https://api.anthropic.com/v1/messages"));
    }

    #[test]
    fn a_file_that_is_not_the_pinned_one_leaves_nothing() {
        let dir = tmp("tampered");
        let files = MODEL_FILES
            .iter()
            .map(|f| (file_url(f.name), b"not the model".to_vec()))
            .collect();
        let t = Table {
            files,
            asked: Default::default(),
        };
        let r = download(&t, &dir, &|_| {});
        assert_eq!(r, Err(ModelError::Tampered("config.json".to_owned())));
        assert!(fs::read_dir(&dir).map(|d| d.count()).unwrap_or(0) == 0);
        assert!(!present(&dir));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_larger_body_is_cut_and_refused() {
        let seen = Cell::new(0u64);
        let mut out = Vec::new();
        let r = copy_limited(&[0u8; 10][..], &mut out, 4, &|n| seen.set(n));
        assert!(matches!(r, Err(ModelError::Tampered(_))));
        assert!(out.len() <= 4);
    }

    #[test]
    fn an_intact_file_is_not_fetched_again_and_the_wrong_one_is() {
        let dir = tmp("resume");
        fs::create_dir_all(&dir).ok();
        // config.json written with a wrong body: it must be fetched (and here refused).
        fs::write(dir.join("config.json"), b"{}").ok();
        let t = Table {
            files: HashMap::new(),
            asked: Default::default(),
        };
        assert_eq!(download(&t, &dir, &|_| {}), Err(ModelError::Server(404)));
        assert_eq!(t.asked.borrow().as_slice(), [file_url("config.json")]);
        // The wrong file stays as it was (never used: `verify` refuses it).
        assert!(verify(&dir).is_err());
        let _ = fs::remove_dir_all(&dir);
    }
}
