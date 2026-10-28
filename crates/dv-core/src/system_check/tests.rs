//! "בדיקת מערכת" (D-142): each check's classification, with stand-ins for the worker, the
//! engine and the network. Nothing here opens a connection or starts the app's reader.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::cell::RefCell;

use super::*;

type ReadFn = dyn Fn(&str) -> Result<Outcome, IngestError>;

/// Stand-ins for everything outside the program.
struct Fake {
    read: Box<ReadFn>,
    ocr: Result<String, IngestError>,
    reach: Reach,
    os: Option<String>,
    free: Option<u64>,
    models: RefCell<Vec<String>>,
    files: RefCell<Vec<String>>,
}

impl Default for Fake {
    fn default() -> Self {
        Self {
            // The real parser, in this process: what the isolated reader runs.
            read: Box::new(|_| panic!("no reading expected")),
            ocr: Ok(String::new()),
            reach: Reach::Reached,
            os: None,
            free: None,
            models: RefCell::default(),
            files: RefCell::default(),
        }
    }
}

impl Probes for Fake {
    fn read(&self, _: &Path, file_name: &str, bytes: &[u8]) -> Result<Outcome, IngestError> {
        self.files.borrow_mut().push(file_name.to_owned());
        let _ = bytes;
        (self.read)(file_name)
    }
    fn ocr(&self, _: &Engine, page: &PageImage) -> Result<String, IngestError> {
        assert!(page.bytes.starts_with(b"\x89PNG"));
        self.ocr.clone()
    }
    fn reach(&self, model: &str) -> Reach {
        self.models.borrow_mut().push(model.to_owned());
        self.reach.clone()
    }
    fn os_version(&self) -> Option<String> {
        self.os.clone()
    }
    fn free_bytes(&self, _: &Path) -> Option<u64> {
        self.free
    }
}

fn job(check: SystemCheck, inputs: Inputs) -> SystemCheckJob {
    SystemCheckJob { check, inputs }
}

fn worker() -> Option<PathBuf> {
    Some(PathBuf::from("reader-that-is-never-started"))
}

/// Every line can be copied for Or: ASCII details, no path, a next step on every failure.
fn well_formed(lines: &[CheckLine]) {
    for l in lines {
        assert!(l.detail.is_ascii(), "{l:?}");
        assert!(!l.detail.contains('/') && !l.detail.contains('\\'), "{l:?}");
        assert!(l.key.is_ascii() && !l.key.is_empty(), "{l:?}");
        assert!(!l.label.is_empty() && !l.reason.is_empty(), "{l:?}");
        if l.state == CheckState::Failed {
            assert!(
                l.next_step.is_some(),
                "a failure without a next step: {l:?}"
            );
        }
    }
}

fn read_with(f: impl Fn(&str) -> Result<Outcome, IngestError> + 'static) -> Fake {
    Fake {
        read: Box::new(f),
        ..Fake::default()
    }
}

#[test]
fn the_made_up_files_are_read_by_the_real_parser() {
    // The same parser the isolated reader runs, here in this process.
    let fake = read_with(|name| {
        let s = samples().into_iter().find(|s| s.file_name == name).unwrap();
        dv_ingest::read(&s.bytes, name)
    });
    let r = job(SystemCheck::Reading, Inputs::Reading { worker: worker() }).run_with(&fake);
    well_formed(&r.lines);
    let keys: Vec<&str> = r.lines.iter().map(|l| l.key.as_str()).collect();
    assert_eq!(keys, ["docx", "pdf", "image"]);
    for l in &r.lines {
        assert_eq!(l.state, CheckState::Ok, "{l:?}");
    }
    assert_eq!(
        *fake.files.borrow(),
        ["system-check.docx", "system-check.pdf", "system-check.png"]
    );
}

#[test]
fn the_samples_hold_nothing_but_their_own_words() {
    for s in samples() {
        assert!(
            s.bytes.len() < 64 * 1024,
            "{} is {} bytes",
            s.key,
            s.bytes.len()
        );
    }
    let pdf = String::from_utf8_lossy(&sample_pdf()).into_owned();
    for l in PDF_LINES {
        assert!(pdf.contains(l));
    }
    // Several lines, so the text layer is not taken for a scan.
    let letters: usize = PDF_LINES
        .iter()
        .map(|l| l.chars().filter(|c| c.is_alphabetic()).count())
        .sum();
    assert!(letters > 100);
    let png = sample_png();
    assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"));
    assert!(png.ends_with(&[0xAE, 0x42, 0x60, 0x82]), "IEND's CRC");
}

#[test]
fn a_reader_that_cannot_start_says_its_code_and_what_to_do() {
    let fake = read_with(|_| Err(IngestError::WorkerCrashed(0xC000_0135)));
    let r = job(SystemCheck::Reading, Inputs::Reading { worker: worker() }).run_with(&fake);
    well_formed(&r.lines);
    assert_eq!(r.lines.len(), 3);
    for l in &r.lines {
        assert_eq!(l.state, CheckState::Failed);
        assert!(l.reason.contains("0xc0000135"), "{}", l.reason);
        assert!(l.reason.contains("לא בעיה בקבצים"));
        assert!(l.next_step.as_deref().unwrap().contains("מתקינים מחדש"));
        assert!(
            l.detail.contains("worker stopped: 0xc0000135"),
            "{}",
            l.detail
        );
    }
    // No access to its own files: the antivirus first.
    let denied = read_with(|_| Err(IngestError::WorkerCrashed(0xC000_0022)));
    let r = job(SystemCheck::Reading, Inputs::Reading { worker: worker() }).run_with(&denied);
    assert!(r.lines[0]
        .next_step
        .as_deref()
        .unwrap()
        .contains("אנטי-וירוס"));
    // Could not be isolated at all.
    let isolated = read_with(|_| Err(IngestError::Worker("appcontainer".to_owned())));
    let r = job(SystemCheck::Reading, Inputs::Reading { worker: worker() }).run_with(&isolated);
    assert_eq!(r.lines[0].state, CheckState::Failed);
    assert!(r.lines[0].detail.contains("worker: appcontainer"));
}

#[test]
fn a_reader_that_runs_out_of_time_is_not_waited_for_three_times() {
    let fake = read_with(|_| Err(IngestError::Timeout));
    let r = job(SystemCheck::Reading, Inputs::Reading { worker: worker() }).run_with(&fake);
    well_formed(&r.lines);
    let states: Vec<CheckState> = r.lines.iter().map(|l| l.state).collect();
    assert_eq!(
        states,
        [CheckState::Failed, CheckState::Skipped, CheckState::Skipped]
    );
    assert_eq!(fake.files.borrow().len(), 1);
}

#[test]
fn a_wrong_answer_from_the_reader_is_a_failure_not_a_pass() {
    // Text where a picture was expected, a picture where text was, text that differs.
    let fake = read_with(|name| {
        Ok(if name.ends_with(".pdf") {
            Outcome::Scan(dv_ingest::Scan {
                format: Format::Pdf,
                images: Vec::new(),
                metadata: Vec::new(),
                warnings: Vec::new(),
                pages: 1,
            })
        } else {
            Outcome::Text(dv_ingest::Extracted {
                format: Format::Docx,
                body: "something else".to_owned(),
                margins: String::new(),
                metadata: Vec::new(),
                warnings: Vec::new(),
                pages: 1,
                ocr: false,
            })
        })
    });
    let r = job(SystemCheck::Reading, Inputs::Reading { worker: worker() }).run_with(&fake);
    well_formed(&r.lines);
    assert!(r.lines.iter().all(|l| l.state == CheckState::Failed));
    assert!(r.lines[1].reason.contains("כסריקה"));
}

#[test]
fn without_the_reader_every_file_says_reinstall() {
    let r = job(SystemCheck::Reading, Inputs::Reading { worker: None }).run_with(&Fake::default());
    well_formed(&r.lines);
    assert!(r.lines.iter().all(|l| l.state == CheckState::Failed
        && l.next_step.as_deref().unwrap().contains("מתקינים מחדש")));
}

fn engine() -> Engine {
    Engine::new(
        PathBuf::from("ocr-never-started"),
        PathBuf::from("tessdata"),
    )
}

#[test]
fn ocr_is_skipped_with_a_reason_when_it_is_not_installed() {
    let r = job(SystemCheck::Ocr, Inputs::Ocr { engine: None }).run_with(&Fake::default());
    well_formed(&r.lines);
    assert_eq!(r.lines[0].state, CheckState::Skipped);
    assert!(r.lines[0].reason.contains("לא מותקן"));
    assert!(r.lines[0].next_step.is_some());
}

#[test]
fn ocr_counts_a_page_read_mostly_right_and_warns_on_a_poor_one() {
    let ocr = |text: &str| {
        let fake = Fake {
            ocr: Ok(text.to_owned()),
            ..Fake::default()
        };
        let r = job(
            SystemCheck::Ocr,
            Inputs::Ocr {
                engine: Some(engine()),
            },
        )
        .run_with(&fake);
        well_formed(&r.lines);
        r.lines[0].state
    };
    assert_eq!(ocr("SYSTEM CHECK\n1357"), CheckState::Ok);
    // What the engine really reads off the dot letters, give or take a letter.
    assert_eq!(ocr("S'STEM CHECE\n1357"), CheckState::Ok);
    assert_eq!(ocr("STSTEM CHECK 135°"), CheckState::Ok);
    assert_eq!(ocr("א ב"), CheckState::Warn);
    assert_eq!(ocr(""), CheckState::Warn);
}

#[test]
fn an_ocr_engine_that_stops_says_so() {
    for (e, word) in [
        (
            IngestError::Ocr("engine exited: exit code: 0xc0000135".to_owned()),
            "מתקינים מחדש",
        ),
        (IngestError::Worker("spawn".to_owned()), "אנטי-וירוס"),
        (IngestError::Timeout, "אנטי-וירוס"),
    ] {
        let fake = Fake {
            ocr: Err(e.clone()),
            ..Fake::default()
        };
        let r = job(
            SystemCheck::Ocr,
            Inputs::Ocr {
                engine: Some(engine()),
            },
        )
        .run_with(&fake);
        well_formed(&r.lines);
        assert_eq!(r.lines[0].state, CheckState::Failed, "{e:?}");
        assert!(r.lines[0].next_step.as_deref().unwrap().contains(word));
        assert!(r.lines[0].detail.starts_with("ocr: "));
    }
}

/// The engine of this computer, when it has one with Hebrew (a development machine): it
/// really reads the page picture. A local program, no network.
#[test]
fn the_page_picture_is_readable_by_a_real_engine() {
    let exe = PathBuf::from("/usr/bin/tesseract");
    let data = [
        "/usr/share/tesseract-ocr/5/tessdata",
        "/usr/share/tesseract-ocr/4.00/tessdata",
    ]
    .into_iter()
    .map(PathBuf::from)
    .find(|d| d.join("heb.traineddata").is_file() && d.join("eng.traineddata").is_file());
    let (true, Some(data)) = (exe.is_file(), data) else {
        return;
    };
    let engine = Engine::new(exe, data);
    let text = Real
        .ocr(
            &engine,
            &PageImage {
                page: 1,
                dpi: None,
                bytes: sample_png(),
            },
        )
        .unwrap();
    assert!(ocr_share(&text) >= OCR_ENOUGH, "{text:?}");
}

fn leftovers(dir: &Path) -> Vec<String> {
    std::fs::read_dir(dir)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect()
}

#[test]
fn a_folder_that_takes_the_trial_is_ok_and_nothing_is_left_behind() {
    let vault = tempfile::tempdir().unwrap();
    let downloads = tempfile::tempdir().unwrap();
    let backup = tempfile::tempdir().unwrap();
    std::fs::write(downloads.path().join("existing.docx"), b"hers").unwrap();
    let r = job(
        SystemCheck::Folders,
        Inputs::Folders {
            export: vec![(ExportPlace::Downloads, downloads.path().to_path_buf())],
            backup: BackupFolder::At(backup.path().to_path_buf()),
            vault_dir: vault.path().to_path_buf(),
        },
    )
    .run_with(&Fake::default());
    well_formed(&r.lines);
    assert_eq!(r.lines[0].state, CheckState::Ok, "{:?}", r.lines[0]);
    assert!(r.lines[0].reason.contains("ההורדות"));
    assert!(r.lines[0].detail.contains("downloads"));
    assert_eq!(r.lines[1].state, CheckState::Ok, "{:?}", r.lines[1]);
    assert_eq!(leftovers(downloads.path()), ["existing.docx"]);
    assert_eq!(
        std::fs::read(downloads.path().join("existing.docx")).unwrap(),
        b"hers"
    );
    assert!(leftovers(backup.path()).is_empty());
    assert!(leftovers(vault.path()).is_empty());
}

#[test]
fn a_synced_downloads_folder_is_passed_over_as_saving_does() {
    let root = tempfile::tempdir().unwrap();
    let synced = root.path().join("OneDrive").join("Downloads");
    std::fs::create_dir_all(&synced).unwrap();
    let documents = root.path().join("Documents");
    std::fs::create_dir_all(&documents).unwrap();
    let vault = root.path().join("vault");
    std::fs::create_dir_all(&vault).unwrap();
    let line = export_folder(
        &[
            (ExportPlace::Downloads, synced.clone()),
            (ExportPlace::Documents, documents.clone()),
        ],
        &vault,
    );
    assert_eq!(line.state, CheckState::Ok, "{line:?}");
    assert!(line.reason.contains("מסונכרנת לענן"), "{}", line.reason);
    assert!(line.detail.contains("documents"));
    assert!(leftovers(&synced).is_empty());
    // Nothing usual left: she is told she will choose the folder herself.
    let none = export_folder(&[(ExportPlace::Downloads, synced)], &vault);
    assert_eq!(none.state, CheckState::Warn);
    assert!(none.detail.contains("synced: downloads"), "{}", none.detail);
    let missing = export_folder(
        &[(ExportPlace::Documents, root.path().join("not-there"))],
        &vault,
    );
    assert_eq!(missing.state, CheckState::Warn);
}

#[test]
fn nothing_is_written_inside_the_vault_folder() {
    let vault = tempfile::tempdir().unwrap();
    let inner = vault.path().join("inner");
    std::fs::create_dir_all(&inner).unwrap();
    for dir in [vault.path().to_path_buf(), inner.clone()] {
        let line = backup_folder(&BackupFolder::At(dir.clone()), vault.path());
        assert_eq!(line.state, CheckState::Skipped, "{line:?}");
        let line = export_folder(&[(ExportPlace::Home, dir)], vault.path());
        assert_eq!(line.state, CheckState::Skipped, "{line:?}");
    }
    assert_eq!(leftovers(vault.path()), ["inner"]);
    assert!(leftovers(&inner).is_empty());
}

#[test]
fn the_backup_folder_is_told_apart_locked_never_missing_and_synced() {
    let vault = tempfile::tempdir().unwrap();
    let cases = [
        (BackupFolder::Locked, CheckState::Skipped),
        (BackupFolder::Never, CheckState::Skipped),
        (
            BackupFolder::At(vault.path().join("unplugged-drive")),
            CheckState::Warn,
        ),
        (
            BackupFolder::At(PathBuf::from("/home/x/Dropbox/backups")),
            CheckState::Warn,
        ),
    ];
    for (folder, state) in cases {
        let line = backup_folder(&folder, vault.path());
        assert_eq!(line.state, state, "{folder:?}");
        well_formed(std::slice::from_ref(&line));
    }
}

#[test]
fn a_refused_write_names_controlled_folder_access_and_never_the_path() {
    use std::io::{Error, ErrorKind};
    let path_error = || Error::new(ErrorKind::PermissionDenied, r"C:\Users\x\Documents\secret");
    let l = write_failure(
        "export_folder",
        "שמירת דוחות",
        "תיקיית המסמכים",
        Step::Create,
        &path_error(),
    );
    assert_eq!(l.state, CheckState::Failed);
    assert!(l
        .next_step
        .as_deref()
        .unwrap()
        .contains("גישה מבוקרת לתיקיות"));
    assert!(
        l.detail.starts_with("export_folder: create"),
        "{}",
        l.detail
    );
    assert!(!l.detail.contains("Users"), "{}", l.detail);
    for (kind, word) in [
        (ErrorKind::StorageFull, "אין מקום"),
        (ErrorKind::NotFound, "לא נמצאה"),
        (ErrorKind::Other, "נכשלה"),
    ] {
        let l = write_failure(
            "backup_folder",
            "שמירת גיבוי",
            "תיקיית הגיבוי",
            Step::Write,
            &Error::from(kind),
        );
        assert_eq!(l.state, CheckState::Failed);
        assert!(l.reason.contains(word), "{kind:?}: {}", l.reason);
        well_formed(std::slice::from_ref(&l));
    }
    // Written, but the trial file stayed: works, and she is told its name only.
    let l = write_failure(
        "backup_folder",
        "שמירת גיבוי",
        "תיקיית הגיבוי",
        Step::Delete,
        &path_error(),
    );
    assert_eq!(l.state, CheckState::Warn);
    assert!(l.reason.contains("dv-write-check"));
}

#[test]
fn a_folder_that_cannot_be_written_fails_the_trial() {
    let vault = tempfile::tempdir().unwrap();
    let gone = vault
        .path()
        .parent()
        .unwrap()
        .join("dv-check-no-such-folder-x");
    match trial_write(&gone) {
        Err((Step::Create, e)) => assert_eq!(e.kind(), std::io::ErrorKind::NotFound),
        other => panic!("{other:?}"),
    }
}

#[test]
fn every_way_the_connection_ends_has_its_own_line() {
    let model = || Some((crate::ALLOWED_MODELS[0].to_owned(), "Claude"));
    let cases = [
        (Reach::Reached, CheckState::Ok, ""),
        (
            Reach::NoName("dns: Other".into()),
            CheckState::Failed,
            "DNS",
        ),
        (
            Reach::NoNetwork("connect: ConnectionRefused".into()),
            CheckState::Failed,
            "חומת אש",
        ),
        (
            Reach::TimedOut("handshake: TimedOut".into()),
            CheckState::Failed,
            "בזמן",
        ),
        (
            Reach::Intercepted("certificate: unknown issuer".into()),
            CheckState::Failed,
            "אנטי-וירוס",
        ),
        (Reach::Pinned, CheckState::Failed, "רשות"),
        (
            Reach::Tls("tls: closed".into()),
            CheckState::Failed,
            "proxy",
        ),
        (Reach::Stopped, CheckState::Skipped, "עצירת השליחה"),
        (Reach::Local, CheckState::Skipped, "על המחשב הזה"),
        (Reach::NotAllowed, CheckState::Failed, "לא ברשימה"),
    ];
    for (reach, state, word) in cases {
        let fake = Fake {
            reach: reach.clone(),
            ..Fake::default()
        };
        let r = job(
            SystemCheck::Connection,
            Inputs::Connection { model: model() },
        )
        .run_with(&fake);
        well_formed(&r.lines);
        let l = &r.lines[0];
        assert_eq!(l.state, state, "{reach:?}");
        assert!(l.reason.contains(word), "{reach:?}: {}", l.reason);
        assert_eq!(l.label, "חיבור ל-Claude");
        assert!(l.detail.starts_with("connection: "));
        assert_eq!(*fake.models.borrow(), [crate::ALLOWED_MODELS[0]]);
    }
    // The interception names the certificate's fault, never what the certificate says.
    let fake = Fake {
        reach: Reach::Intercepted("certificate: unknown issuer".into()),
        ..Fake::default()
    };
    let r = job(
        SystemCheck::Connection,
        Inputs::Connection { model: model() },
    )
    .run_with(&fake);
    assert_eq!(r.lines[0].detail, "connection: certificate: unknown issuer");
    assert!(r.lines[0].next_step.as_deref().unwrap().contains("HTTPS"));
}

#[test]
fn a_locked_vault_opens_no_connection() {
    let fake = Fake::default();
    let r = job(SystemCheck::Connection, Inputs::Connection { model: None }).run_with(&fake);
    assert_eq!(r.lines[0].state, CheckState::Skipped);
    assert!(fake.models.borrow().is_empty());
}

fn basics(b: Basics, fake: &Fake) -> Vec<CheckLine> {
    let vault = tempfile::tempdir().unwrap();
    let r = job(
        SystemCheck::Basics,
        Inputs::Basics {
            basics: b,
            vault_dir: vault.path().to_path_buf(),
        },
    )
    .run_with(fake);
    well_formed(&r.lines);
    r.lines
}

fn state_of(lines: &[CheckLine], key: &str) -> CheckState {
    lines.iter().find(|l| l.key == key).unwrap().state
}

#[test]
fn the_basics_say_key_model_version_system_fips_and_disk() {
    let default = || Basics {
        unlocked: true,
        local: false,
        has_key: true,
        stored_model: None,
        model: crate::ALLOWED_MODELS[0].to_owned(),
    };
    let fake = Fake {
        os: Some("Windows 10.0.22631.0".to_owned()),
        free: Some(40 * GIB),
        ..Fake::default()
    };
    let lines = basics(default(), &fake);
    let keys: Vec<&str> = lines.iter().map(|l| l.key.as_str()).collect();
    assert_eq!(keys, ["api_key", "model", "version", "os", "fips", "disk"]);
    assert_eq!(state_of(&lines, "api_key"), CheckState::Ok);
    assert_eq!(state_of(&lines, "model"), CheckState::Ok);
    assert_eq!(state_of(&lines, "disk"), CheckState::Ok);
    let os = lines.iter().find(|l| l.key == "os").unwrap();
    assert!(os.reason.contains("Windows 11"), "{}", os.reason);
    assert!(lines
        .iter()
        .find(|l| l.key == "version")
        .unwrap()
        .detail
        .contains(env!("CARGO_PKG_VERSION")));
    // Never the key itself, never the model's name.
    for l in &lines {
        assert!(!l.detail.contains(crate::ALLOWED_MODELS[0]), "{l:?}");
    }

    let no_key = basics(
        Basics {
            has_key: false,
            ..default()
        },
        &Fake::default(),
    );
    assert_eq!(state_of(&no_key, "api_key"), CheckState::Warn);
    assert_eq!(state_of(&no_key, "os"), CheckState::Skipped);
    assert_eq!(state_of(&no_key, "disk"), CheckState::Skipped);
    let local = basics(
        Basics {
            local: true,
            has_key: false,
            ..default()
        },
        &Fake::default(),
    );
    assert_eq!(state_of(&local, "api_key"), CheckState::Ok);
    let old = basics(
        Basics {
            stored_model: Some("a-model-that-left-the-list".to_owned()),
            ..default()
        },
        &Fake::default(),
    );
    assert_eq!(state_of(&old, "model"), CheckState::Warn);
    let locked = basics(
        Basics {
            unlocked: false,
            has_key: false,
            ..default()
        },
        &Fake::default(),
    );
    assert_eq!(state_of(&locked, "api_key"), CheckState::Skipped);
}

#[test]
fn little_room_on_the_disk_is_said_before_it_runs_out() {
    assert_eq!(disk_line(Some(300 * MIB)).state, CheckState::Failed);
    assert_eq!(disk_line(Some(3 * GIB)).state, CheckState::Warn);
    assert_eq!(disk_line(Some(50 * GIB)).state, CheckState::Ok);
    assert_eq!(disk_line(None).state, CheckState::Skipped);
    assert_eq!(disk_line(Some(3 * GIB)).detail, "disk: 3072 MiB free");
}

#[test]
fn details_keep_nothing_after_a_path_and_nothing_but_ascii() {
    assert_eq!(
        ascii("ocr: engine at C:\\Program Files\\x"),
        "ocr: engine at C:<path left out>"
    );
    assert_eq!(ascii("dns: שגיאה Other"), "dns:  Other");
    assert_eq!(ascii(&"x".repeat(500)).len(), 200);
}

#[test]
fn the_core_hands_out_only_what_each_check_needs_locked_or_open() {
    let dir = tempfile::tempdir().unwrap();
    let mut core = Core::for_tests(dir.path(), None);
    // Locked (no vault yet): nothing about her settings is known, nothing is guessed.
    let locked = core.begin_system_check(SystemCheck::Folders, Vec::new());
    assert!(matches!(
        locked.inputs,
        Inputs::Folders {
            backup: BackupFolder::Locked,
            ..
        }
    ));
    let connection = core.begin_system_check(SystemCheck::Connection, Vec::new());
    assert!(matches!(
        connection.inputs,
        Inputs::Connection { model: None }
    ));

    core.create_vault("כלב ירוק רץ מהר בגינה").unwrap();
    let open = core.begin_system_check(SystemCheck::Folders, Vec::new());
    assert!(matches!(
        open.inputs,
        Inputs::Folders {
            backup: BackupFolder::Never,
            ..
        }
    ));
    let connection = core.begin_system_check(SystemCheck::Connection, Vec::new());
    assert!(matches!(
        &connection.inputs,
        Inputs::Connection { model: Some((m, "Claude")) } if crate::ALLOWED_MODELS.contains(&m.as_str())
    ));
    let Inputs::Basics { basics, .. } = core
        .begin_system_check(SystemCheck::Basics, Vec::new())
        .inputs
    else {
        panic!("basics")
    };
    assert!(basics.unlocked && !basics.has_key && !basics.local);
    core.set_api_key("anthropic", "sk-test-not-a-real-key", false)
        .unwrap();
    let Inputs::Basics { basics, .. } = core
        .begin_system_check(SystemCheck::Basics, Vec::new())
        .inputs
    else {
        panic!("basics")
    };
    assert!(basics.has_key);
    // The job carries no key.
    let job = core.begin_system_check(SystemCheck::Basics, Vec::new());
    assert!(!format!("{job:?}").contains("sk-test"));
}
