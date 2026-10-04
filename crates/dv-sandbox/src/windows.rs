//! The Windows calls behind [`crate::spawn`]: an AppContainer with no capabilities, inside a
//! job object. The audited `unsafe` of the workspace (D-044); each block says why it is sound.
//!
//! Order matters: the process is created suspended, put in the job, and only then resumed, so
//! no instruction of it runs outside the job. Only its own two pipe ends are inherited
//! (`PROC_THREAD_ATTRIBUTE_HANDLE_LIST`), never another handle of the app.

use std::collections::HashSet;
use std::ffi::{c_void, OsStr, OsString};
use std::fs::File;
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::os::windows::process::{CommandExt, ExitStatusExt};
use std::path::{Path, PathBuf};
use std::process::ExitStatus;
use std::ptr::{null, null_mut};
use std::sync::Mutex;
use std::time::Duration;

use windows_sys::core::{HRESULT, PWSTR};
use windows_sys::Win32::Foundation::{
    LocalFree, SetHandleInformation, HANDLE, HANDLE_FLAG_INHERIT, HLOCAL, WAIT_OBJECT_0,
    WAIT_TIMEOUT,
};
use windows_sys::Win32::Security::Authorization::ConvertSidToStringSidW;
use windows_sys::Win32::Security::Isolation::{
    CreateAppContainerProfile, DeriveAppContainerSidFromAppContainerName,
};
use windows_sys::Win32::Security::{FreeSid, PSID, SECURITY_ATTRIBUTES, SECURITY_CAPABILITIES};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectBasicUIRestrictions,
    JobObjectExtendedLimitInformation, SetInformationJobObject, TerminateJobObject,
    JOBOBJECT_BASIC_UI_RESTRICTIONS, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JOB_OBJECT_LIMIT_ACTIVE_PROCESS, JOB_OBJECT_LIMIT_DIE_ON_UNHANDLED_EXCEPTION,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOB_OBJECT_LIMIT_PROCESS_MEMORY,
};
use windows_sys::Win32::System::Pipes::CreatePipe;
use windows_sys::Win32::System::Threading::{
    CreateProcessW, DeleteProcThreadAttributeList, GetExitCodeProcess,
    InitializeProcThreadAttributeList, ResumeThread, TerminateProcess, UpdateProcThreadAttribute,
    WaitForSingleObject, CREATE_NO_WINDOW, CREATE_SUSPENDED, CREATE_UNICODE_ENVIRONMENT,
    EXTENDED_STARTUPINFO_PRESENT, PROCESS_INFORMATION, PROC_THREAD_ATTRIBUTE_HANDLE_LIST,
    PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES, STARTF_USESTDHANDLES, STARTUPINFOEXW,
};

use crate::Policy;

/// The container's name. One profile for every worker; it holds nothing.
const CONTAINER: &str = "DiagnosticVault.DocumentReader";
/// `HRESULT_FROM_WIN32(ERROR_ALREADY_EXISTS)`.
const ALREADY_EXISTS: HRESULT = 0x8007_00B7_u32 as HRESULT;
/// All of `JOB_OBJECT_UILIMIT_*`: clipboard, desktop, display settings, global atoms, other
/// processes' windows, shutting down, system parameters.
const UILIMIT_ALL: u32 = 0x1FF;

pub(crate) struct Contained {
    pub(crate) stdin: Option<File>,
    pub(crate) stdout: Option<File>,
    process: OwnedHandle,
    /// Kill-on-close: dropping the last handle ends the process.
    job: OwnedHandle,
}

fn wide(s: &OsStr) -> Vec<u16> {
    s.encode_wide().chain(Some(0)).collect()
}

fn hresult(hr: HRESULT) -> io::Result<()> {
    if hr >= 0 {
        return Ok(());
    }
    // FACILITY_WIN32 carries a plain error code in the low word.
    if (hr >> 16) & 0x1FFF == 7 {
        Err(io::Error::from_raw_os_error(hr & 0xFFFF))
    } else {
        Err(io::Error::other(format!("HRESULT {hr:#010x}")))
    }
}

fn check(ok: i32) -> io::Result<()> {
    if ok == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// An AppContainer SID, freed on drop.
struct Sid(PSID);

impl Drop for Sid {
    fn drop(&mut self) {
        // SAFETY: the SID came from CreateAppContainerProfile or
        // DeriveAppContainerSidFromAppContainerName, which document FreeSid as its release, and
        // it is freed exactly once here.
        unsafe {
            FreeSid(self.0);
        }
    }
}

fn container_sid() -> io::Result<Sid> {
    let name = wide(OsStr::new(CONTAINER));
    let mut sid: PSID = null_mut();
    // SAFETY: `name` is NUL-terminated and outlives the call; no capabilities are passed (null,
    // count 0); `sid` is a valid out-pointer.
    let hr = unsafe {
        CreateAppContainerProfile(
            name.as_ptr(),
            name.as_ptr(),
            name.as_ptr(),
            null(),
            0,
            &mut sid,
        )
    };
    if hr == ALREADY_EXISTS {
        // SAFETY: as above; the profile exists, so only its SID is derived.
        hresult(unsafe { DeriveAppContainerSidFromAppContainerName(name.as_ptr(), &mut sid) })?;
    } else {
        hresult(hr)?;
    }
    if sid.is_null() {
        return Err(io::Error::other("no AppContainer SID"));
    }
    Ok(Sid(sid))
}

fn sid_string(sid: &Sid) -> io::Result<String> {
    let mut text: PWSTR = null_mut();
    // SAFETY: `sid.0` is a valid SID for the lifetime of `sid`; `text` is a valid out-pointer.
    check(unsafe { ConvertSidToStringSidW(sid.0, &mut text) })?;
    let mut len = 0usize;
    // SAFETY: on success `text` points to a NUL-terminated UTF-16 string; reading stops at the
    // NUL, so every read is inside it.
    while unsafe { *text.add(len) } != 0 {
        len += 1;
    }
    // SAFETY: `len` units were just read from `text` above.
    let units = unsafe { std::slice::from_raw_parts(text, len) };
    let out = String::from_utf16_lossy(units);
    // SAFETY: the string was allocated by ConvertSidToStringSidW with LocalAlloc and is not used
    // after this point.
    unsafe {
        LocalFree(text as HLOCAL);
    }
    Ok(out)
}

/// Paths already granted to the container in this run (an update replaces the program and
/// restarts the app, so once per run is enough).
static GRANTED: Mutex<Option<HashSet<PathBuf>>> = Mutex::new(None);

/// Let the container read and run `path` (a file) or read inside it (a folder). Through the
/// system's own `icacls`, without native calls.
fn grant(path: &Path, sid: &str) -> io::Result<()> {
    let mut granted = GRANTED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let set = granted.get_or_insert_with(HashSet::new);
    if set.contains(path) {
        return Ok(());
    }
    let rights = if path.is_dir() {
        "(OI)(CI)(RX)"
    } else {
        "(RX)"
    };
    let icacls = system32().join("icacls.exe");
    let status = std::process::Command::new(icacls)
        .arg(path)
        .args(["/grant", &format!("*{sid}:{rights}"), "/Q"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .status()?;
    if !status.success() {
        return Err(io::Error::other(format!("icacls: {status}")));
    }
    set.insert(path.to_path_buf());
    Ok(())
}

fn system_root() -> OsString {
    std::env::var_os("SystemRoot").unwrap_or_else(|| OsString::from(r"C:\Windows"))
}

fn system32() -> PathBuf {
    PathBuf::from(system_root()).join("System32")
}

/// An anonymous pipe, both ends inheritable: `(read, write)`.
fn pipe() -> io::Result<(OwnedHandle, OwnedHandle)> {
    let attributes = SECURITY_ATTRIBUTES {
        nLength: size_u32::<SECURITY_ATTRIBUTES>(),
        lpSecurityDescriptor: null_mut(),
        bInheritHandle: 1,
    };
    let (mut read, mut write): (HANDLE, HANDLE) = (null_mut(), null_mut());
    // SAFETY: both out-pointers are valid; `attributes` outlives the call.
    check(unsafe { CreatePipe(&mut read, &mut write, &attributes, 0) })?;
    // SAFETY: CreatePipe succeeded, so both are new handles owned by nobody else.
    Ok(unsafe {
        (
            OwnedHandle::from_raw_handle(read),
            OwnedHandle::from_raw_handle(write),
        )
    })
}

/// The app's own end of a pipe must not be inherited.
fn keep(handle: &OwnedHandle) -> io::Result<()> {
    // SAFETY: a valid handle, borrowed for the call.
    check(unsafe { SetHandleInformation(handle.as_raw_handle(), HANDLE_FLAG_INHERIT, 0) })
}

fn size_u32<T>() -> u32 {
    u32::try_from(std::mem::size_of::<T>()).unwrap_or(u32::MAX)
}

fn job(memory_mb: u32) -> io::Result<OwnedHandle> {
    // SAFETY: no attributes and no name (null pointers are allowed for both).
    let raw = unsafe { CreateJobObjectW(null(), null()) };
    if raw.is_null() {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: a new handle owned by nobody else.
    let job = unsafe { OwnedHandle::from_raw_handle(raw) };

    let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
        | JOB_OBJECT_LIMIT_ACTIVE_PROCESS
        | JOB_OBJECT_LIMIT_PROCESS_MEMORY
        | JOB_OBJECT_LIMIT_DIE_ON_UNHANDLED_EXCEPTION;
    limits.BasicLimitInformation.ActiveProcessLimit = 1;
    limits.ProcessMemoryLimit = usize::try_from(memory_mb)
        .unwrap_or(usize::MAX)
        .saturating_mul(1024 * 1024);
    // SAFETY: `limits` is the structure this information class expects, with its exact size,
    // and outlives the call.
    check(unsafe {
        SetInformationJobObject(
            job.as_raw_handle(),
            JobObjectExtendedLimitInformation,
            (&raw const limits).cast::<c_void>(),
            size_u32::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>(),
        )
    })?;
    let ui = JOBOBJECT_BASIC_UI_RESTRICTIONS {
        UIRestrictionsClass: UILIMIT_ALL,
    };
    // SAFETY: as above, for the UI restrictions class.
    check(unsafe {
        SetInformationJobObject(
            job.as_raw_handle(),
            JobObjectBasicUIRestrictions,
            (&raw const ui).cast::<c_void>(),
            size_u32::<JOBOBJECT_BASIC_UI_RESTRICTIONS>(),
        )
    })?;
    Ok(job)
}

/// An initialized attribute list in an 8-byte-aligned buffer, deleted on drop.
struct Attributes {
    buffer: Vec<u64>,
}

impl Attributes {
    fn new(count: u32) -> io::Result<Self> {
        let mut size = 0usize;
        // SAFETY: a size query: a null list is allowed and only `size` is written. It returns
        // failure (insufficient buffer) by design.
        unsafe {
            InitializeProcThreadAttributeList(null_mut(), count, 0, &mut size);
        }
        let mut buffer = vec![0u64; size.div_ceil(8).max(1)];
        // SAFETY: `buffer` holds at least `size` bytes and is 8-byte aligned.
        check(unsafe {
            InitializeProcThreadAttributeList(buffer.as_mut_ptr().cast(), count, 0, &mut size)
        })?;
        Ok(Self { buffer })
    }

    fn list(&mut self) -> *mut c_void {
        self.buffer.as_mut_ptr().cast()
    }

    /// # Safety
    /// `value` must point to `size` readable bytes that stay valid until the list is used by
    /// CreateProcessW (the caller keeps them alive across that call).
    unsafe fn set(&mut self, attribute: u32, value: *const c_void, size: usize) -> io::Result<()> {
        let list = self.list();
        // SAFETY: `list` was initialized in `new`; the caller guarantees `value`/`size`.
        check(unsafe {
            UpdateProcThreadAttribute(list, 0, attribute as usize, value, size, null_mut(), null())
        })
    }
}

impl Drop for Attributes {
    fn drop(&mut self) {
        let list = self.list();
        // SAFETY: initialized in `new`, deleted once, before the buffer is freed.
        unsafe { DeleteProcThreadAttributeList(list) }
    }
}

/// A command line of the program and simple arguments, each quoted.
fn command_line(exe: &Path, args: &[&str]) -> io::Result<Vec<u16>> {
    let mut line = OsString::new();
    for (i, part) in std::iter::once(exe.as_os_str())
        .chain(args.iter().map(OsStr::new))
        .enumerate()
    {
        if part.encode_wide().any(|c| c == u16::from(b'"')) {
            return Err(io::Error::other("quote in a worker argument"));
        }
        if i > 0 {
            line.push(" ");
        }
        line.push("\"");
        line.push(part);
        line.push("\"");
    }
    Ok(wide(&line))
}

/// `SystemRoot=…\0WINDIR=…\0\0`.
fn environment() -> Vec<u16> {
    let root = system_root();
    let mut block = Vec::new();
    for key in ["SystemRoot", "WINDIR"] {
        let mut entry = OsString::from(key);
        entry.push("=");
        entry.push(&root);
        block.extend(entry.encode_wide());
        block.push(0);
    }
    block.push(0);
    block
}

pub(crate) fn spawn(exe: &Path, args: &[&str], policy: &Policy) -> io::Result<Contained> {
    let sid = container_sid()?;
    let sid_text = sid_string(&sid)?;
    grant(exe, &sid_text)?;
    for dir in &policy.read_dirs {
        grant(dir, &sid_text)?;
    }

    let (stdin_read, stdin_write) = pipe()?;
    let (stdout_read, stdout_write) = pipe()?;
    keep(&stdin_write)?;
    keep(&stdout_read)?;
    let job = job(policy.memory_mb)?;

    let mut attributes = Attributes::new(2)?;
    let capabilities = SECURITY_CAPABILITIES {
        AppContainerSid: sid.0,
        Capabilities: null_mut(),
        CapabilityCount: 0,
        Reserved: 0,
    };
    let inherited: [HANDLE; 2] = [stdin_read.as_raw_handle(), stdout_write.as_raw_handle()];
    // SAFETY: `capabilities` and `inherited` live until after CreateProcessW below; the sizes
    // are theirs exactly.
    unsafe {
        attributes.set(
            PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES,
            (&raw const capabilities).cast(),
            std::mem::size_of::<SECURITY_CAPABILITIES>(),
        )?;
        attributes.set(
            PROC_THREAD_ATTRIBUTE_HANDLE_LIST,
            inherited.as_ptr().cast(),
            std::mem::size_of_val(&inherited),
        )?;
    }

    let mut startup = STARTUPINFOEXW::default();
    startup.StartupInfo.cb = size_u32::<STARTUPINFOEXW>();
    startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
    startup.StartupInfo.hStdInput = stdin_read.as_raw_handle();
    startup.StartupInfo.hStdOutput = stdout_write.as_raw_handle();
    startup.StartupInfo.hStdError = null_mut();
    startup.lpAttributeList = attributes.list();

    let application = wide(exe.as_os_str());
    let mut line = command_line(exe, args)?;
    let environment = environment();
    let folder = wide(system32().as_os_str());
    let mut info = PROCESS_INFORMATION::default();
    // SAFETY: every string is NUL-terminated (the environment double-NUL-terminated, UTF-16 as
    // CREATE_UNICODE_ENVIRONMENT says) and outlives the call; `line` is writable as required;
    // `startup` is a STARTUPINFOEXW as EXTENDED_STARTUPINFO_PRESENT says, its attribute list and
    // everything it points to are alive; `info` is a valid out-pointer.
    check(unsafe {
        CreateProcessW(
            application.as_ptr(),
            line.as_mut_ptr(),
            null(),
            null(),
            1,
            EXTENDED_STARTUPINFO_PRESENT
                | CREATE_SUSPENDED
                | CREATE_NO_WINDOW
                | CREATE_UNICODE_ENVIRONMENT,
            environment.as_ptr().cast(),
            folder.as_ptr(),
            &startup.StartupInfo,
            &mut info,
        )
    })?;
    // SAFETY: CreateProcessW succeeded: two new handles owned by nobody else.
    let (process, thread) = unsafe {
        (
            OwnedHandle::from_raw_handle(info.hProcess),
            OwnedHandle::from_raw_handle(info.hThread),
        )
    };
    drop(attributes);

    // Suspended: nothing has run yet. Into the job first, then resume.
    // SAFETY: both handles are valid and borrowed for the call.
    let assigned =
        check(unsafe { AssignProcessToJobObject(job.as_raw_handle(), process.as_raw_handle()) });
    if let Err(e) = assigned {
        // SAFETY: a valid process handle; it never ran.
        unsafe {
            TerminateProcess(process.as_raw_handle(), 1);
        }
        return Err(e);
    }
    // SAFETY: a valid thread handle, borrowed for the call.
    if unsafe { ResumeThread(thread.as_raw_handle()) } == u32::MAX {
        let e = io::Error::last_os_error();
        // SAFETY: a valid job handle; ends the process in it.
        unsafe {
            TerminateJobObject(job.as_raw_handle(), 1);
        }
        return Err(e);
    }
    // The child's ends now live in the child only.
    drop(stdin_read);
    drop(stdout_write);
    Ok(Contained {
        stdin: Some(File::from(stdin_write)),
        stdout: Some(File::from(stdout_read)),
        process,
        job,
    })
}

impl Contained {
    pub(crate) fn wait_timeout(&mut self, timeout: Duration) -> io::Result<Option<ExitStatus>> {
        let ms = u32::try_from(timeout.as_millis()).unwrap_or(u32::MAX - 1);
        // SAFETY: a valid process handle, borrowed for the call.
        match unsafe { WaitForSingleObject(self.process.as_raw_handle(), ms) } {
            WAIT_OBJECT_0 => {
                let mut code = 0u32;
                // SAFETY: a valid process handle and out-pointer.
                check(unsafe { GetExitCodeProcess(self.process.as_raw_handle(), &mut code) })?;
                Ok(Some(ExitStatus::from_raw(code)))
            }
            WAIT_TIMEOUT => Ok(None),
            _ => Err(io::Error::last_os_error()),
        }
    }

    pub(crate) fn kill(&mut self) {
        // SAFETY: a valid job handle, borrowed for the call.
        unsafe {
            TerminateJobObject(self.job.as_raw_handle(), 1);
        }
    }
}
