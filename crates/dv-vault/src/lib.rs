//! The encrypted vault (docs/ARCHITECTURE.md → "מפתחות", "סכמה").
//!
//! * Master key (MK) wrapped in LUKS-style key slots (password, printed recovery key; Windows
//!   Hello / Touch ID later) in an authenticated header.
//! * Three SQLCipher databases (`main`, `identity`, `audit`) with keys derived from MK.
//! * Inside them, every case value is sealed with the case's own key (AES-256-GCM, AAD bound
//!   to table/column/row/case), so deleting a case is cryptographic shredding.
//! * Tamper-evident audit log (HMAC chain + anchor in the header), metadata only.

mod audit;
pub mod crypto;
mod db;
pub mod env;
mod header;
pub mod password;
pub mod recovery;
mod store;

pub use audit::{AuditEntry, AuditEvent};
pub use password::{Argon2Params, PolicyViolation, MIN_PASSWORD_CHARS};
pub use store::{Created, IntegrityReport, Practitioner, Vault};

#[derive(Debug, thiserror::Error)]
pub enum VaultError {
    #[error("wrong password or recovery key")]
    WrongSecret,
    #[error("the recovery key is not valid (check for typos)")]
    RecoveryKeyInvalid,
    #[error("password does not meet the policy: {0:?}")]
    Policy(PolicyViolation),
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
