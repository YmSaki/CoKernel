use super::*;
use sha2::{Digest, Sha256};
use std::{path::Path, ptr};
use windows_sys::Win32::{
    Foundation::{ERROR_NOT_FOUND, GetLastError},
    Security::Credentials::*,
};
use zeroize::Zeroize;

/// Exactly one application-scoped credential. Never enumerates other credentials.
pub struct WindowsSecretStore {
    target: Vec<u16>,
}
impl WindowsSecretStore {
    pub fn for_state(directory: &Path) -> Result<Self> {
        use std::os::windows::ffi::OsStrExt;
        let canonical = directory
            .canonicalize()
            .map_err(|_| SecretError::Unavailable)?;
        let mut digest = Sha256::new();
        for unit in canonical.as_os_str().encode_wide() {
            digest.update(unit.to_le_bytes());
        }
        let name = format!("CoKernel/v1/{:x}/tunnel", digest.finalize());
        Ok(Self {
            target: name.encode_utf16().chain(Some(0)).collect(),
        })
    }
}
struct Credential(*mut CREDENTIALW);
impl Drop for Credential {
    fn drop(&mut self) {
        unsafe {
            let value = &mut *self.0;
            if !value.CredentialBlob.is_null()
                && value.CredentialBlobSize <= CRED_MAX_CREDENTIAL_BLOB_SIZE
            {
                std::slice::from_raw_parts_mut(
                    value.CredentialBlob,
                    value.CredentialBlobSize as usize,
                )
                .zeroize();
            }
            CredFree(self.0.cast());
        }
    }
}
impl SecretStore for WindowsSecretStore {
    fn load(&mut self) -> Result<Option<TunnelCredentials>> {
        let mut raw = ptr::null_mut();
        // Windows owns returned storage; the guard wipes the blob then CredFree's it.
        if unsafe { CredReadW(self.target.as_ptr(), CRED_TYPE_GENERIC, 0, &mut raw) } == 0 {
            return if unsafe { GetLastError() } == ERROR_NOT_FOUND {
                Ok(None)
            } else {
                Err(SecretError::Unavailable)
            };
        }
        if raw.is_null() {
            return Err(SecretError::Corrupt);
        }
        let credential = Credential(raw);
        let value = unsafe { &*credential.0 };
        if value.Type != CRED_TYPE_GENERIC
            || value.CredentialBlobSize > 2560
            || value.CredentialBlobSize < 7
            || value.CredentialBlob.is_null()
        {
            return Err(SecretError::Corrupt);
        }
        let bytes = unsafe {
            std::slice::from_raw_parts(value.CredentialBlob, value.CredentialBlobSize as usize)
        };
        TunnelCredentials::decode(bytes).map(Some)
    }
    fn replace(&mut self, credentials: &TunnelCredentials) -> Result<()> {
        let mut bytes = credentials.encode();
        let value = CREDENTIALW {
            Type: CRED_TYPE_GENERIC,
            TargetName: self.target.as_mut_ptr(),
            CredentialBlobSize: bytes.len() as u32,
            CredentialBlob: bytes.as_mut_ptr(),
            Persist: CRED_PERSIST_LOCAL_MACHINE,
            ..unsafe { std::mem::zeroed() }
        };
        if unsafe { CredWriteW(&value, 0) } == 0 {
            return Err(SecretError::Unavailable);
        }
        Ok(())
    }
    fn clear(&mut self) -> Result<()> {
        if unsafe { CredDeleteW(self.target.as_ptr(), CRED_TYPE_GENERIC, 0) } == 0
            && unsafe { GetLastError() } != ERROR_NOT_FOUND
        {
            return Err(SecretError::Unavailable);
        }
        Ok(())
    }
}
