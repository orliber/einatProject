//! What is left of Windows Hello (D-047, taken out in D-054): removing the key pair of a vault
//! that had it turned on. The slot itself stays readable in old headers (`KeySlot::WindowsHello`)
//! so those vaults keep opening with the password, and it is removed on the first unlock
//! (`Vault::remove_hello_slot`). No `unsafe`: the Windows Runtime calls are safe in the
//! `windows` crate.

/// Remove the Windows Hello key pair named `credential` from this Windows account. Missing,
/// or not on Windows, is fine: without the slot the key pair opened nothing anyway.
pub fn delete_key_pair(credential: &str) {
    #[cfg(windows)]
    {
        let _ = windows::Security::Credentials::KeyCredentialManager::DeleteAsync(
            &windows::core::HSTRING::from(credential),
        )
        .and_then(|op| op.join());
    }
    #[cfg(not(windows))]
    {
        let _ = credential;
    }
}

#[cfg(test)]
mod tests {
    /// Runs on the Windows CI machine too: removing a key pair that does not exist is quiet.
    #[test]
    fn removing_a_missing_key_pair_never_fails() {
        super::delete_key_pair("il.diagnosticvault.test-missing");
    }
}
