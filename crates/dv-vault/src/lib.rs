//! The encrypted vault (docs/ARCHITECTURE.md → "מפתחות", "סכמה").
//!
//! * Master key (MK) wrapped in LUKS-style key slots (password, printed recovery key, Windows
//!   Hello; Touch ID later) in an authenticated header.
//! * Three SQLCipher databases (`main`, `identity`, `audit`) with keys derived from MK.
//! * Inside them, every case value is sealed with the case's own key (AES-256-GCM, AAD bound
//!   to table/column/row/case), so deleting a case is cryptographic shredding.
//! * Tamper-evident audit log (HMAC chain + anchor in the header), metadata only.

mod audit;
pub mod computer_key;
pub mod crypto;
mod db;
pub mod env;
mod header;
pub mod hello;
pub mod password;
pub mod recovery;
mod store;
pub mod version;

pub use audit::{AuditEntry, AuditEvent};
pub use header::VaultHeader;
pub use password::{Argon2Params, PolicyViolation, MIN_PASSWORD_CHARS};
pub use store::{
    drill_backup, peek_backup, BackupCheck, BackupInfo, BackupJob, BackupPeek, BackupWritten,
    Created, DraftVersion, IntegrityReport, NewSlot, Practitioner, RemovedInput, Secret,
    StoredConsultation, StoredStyleProfile, StoredStyleSource, TokenHasher, Vault, WrittenBackup,
    BACKUP_EXTENSION,
};

#[derive(Debug, thiserror::Error)]
pub enum VaultError {
    #[error("wrong password or recovery key")]
    WrongSecret,
    /// The password slot is bound to another computer (D-076): here only the kit opens it.
    #[error("the password opens this vault only on the computer it is bound to")]
    OtherComputer,
    /// Windows did not answer in time when the key kept for this computer was unsealed (a
    /// cold start, an antivirus scan). Not "another computer": she tries again.
    #[error("the computer did not answer in time")]
    Busy,
    #[error("the recovery key is not valid (check for typos)")]
    RecoveryKeyInvalid,
    #[error("password does not meet the policy: {0:?}")]
    Policy(PolicyViolation),
    /// A newer version of the program opened this vault (D-104): this one does not open it
    /// and writes nothing to it. Holds that version when it can be shown (else empty).
    #[error("the vault was opened by a newer version of the program ({0})")]
    NewerVersion(String),
    #[error("a vault already exists here")]
    AlreadyExists,
    #[error("not found")]
    NotFound,
    #[error("refused: {0}")]
    Refused(String),
    #[error("integrity check failed: {0}")]
    Integrity(&'static str),
    #[error("stored data failed authentication")]
    Authentication,
    #[error("cryptographic failure: {0}")]
    Crypto(&'static str),
    #[error("vault data is corrupt: {0}")]
    Corrupt(String),
    #[error("storage error: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("file error: {0}")]
    Io(#[from] std::io::Error),
}
