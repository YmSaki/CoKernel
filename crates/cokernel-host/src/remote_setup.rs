//! Native human credential entry; the Host owner remains the only vault writer.
use crate::secrets::{SecretError, TunnelCredentials};
use std::{ffi::c_void, ptr};
use windows_sys::Win32::{
    Foundation::{ERROR_CANCELLED, ERROR_SUCCESS},
    Security::Credentials::{
        CREDUI_INFOW, CREDUIWIN_GENERIC, CredUIPromptForWindowsCredentialsW,
        CredUnPackAuthenticationBufferW,
    },
    System::Com::CoTaskMemFree,
};
use zeroize::{Zeroize, Zeroizing};

type Result<T> = std::result::Result<T, SecretError>;
// wincred.h: CREDUI_MAX_PASSWORD_LENGTH = 512 / 2. Reject the boundary
// itself as well, so a provider-clipped paste cannot be accepted as a valid key.
const NATIVE_KEY_BOUNDARY: usize = 256;
struct AuthBuffer {
    pointer: *mut c_void,
    length: u32,
}
impl Drop for AuthBuffer {
    fn drop(&mut self) {
        if !self.pointer.is_null() {
            // SAFETY: Windows supplies this allocation and byte count; its API
            // requires wiping the credential blob before CoTaskMemFree.
            unsafe {
                std::slice::from_raw_parts_mut(self.pointer.cast::<u8>(), self.length as usize)
                    .zeroize();
                CoTaskMemFree(self.pointer);
            }
        }
    }
}

/// Blocking UI for an interactive client thread, never an IPC server handler.
/// Cancellation changes neither credentials nor remote intent.
pub fn prompt_credentials() -> Result<Option<TunnelCredentials>> {
    let caption: Vec<u16> = "CoKernel — Remote AI"
        .encode_utf16()
        .chain(Some(0))
        .collect();
    let message: Vec<u16> = "ユーザー名: Tunnel ID (tunnel_...)\nパスワード: Tunnel runtime API key (16〜255文字)\nWindowsのログイン情報は入力しないでください。\n保存後の接続操作は別です。接続中の場合は新しい設定に切り替わります。"
        .encode_utf16().chain(Some(0)).collect();
    let info = CREDUI_INFOW {
        cbSize: std::mem::size_of::<CREDUI_INFOW>() as u32,
        hwndParent: ptr::null_mut(),
        pszMessageText: message.as_ptr(),
        pszCaptionText: caption.as_ptr(),
        hbmBanner: ptr::null_mut(),
    };
    let mut package = 0;
    let mut output = AuthBuffer {
        pointer: ptr::null_mut(),
        length: 0,
    };
    // No input credentials, save checkbox, Windows logon validation or vault
    // persistence. Host alone stores the pair after an accepted configure RPC.
    let status = unsafe {
        CredUIPromptForWindowsCredentialsW(
            &info,
            0,
            &mut package,
            ptr::null(),
            0,
            &mut output.pointer,
            &mut output.length,
            ptr::null_mut(),
            CREDUIWIN_GENERIC,
        )
    };
    finish_prompt(status, &output)
}
fn finish_prompt(status: u32, output: &AuthBuffer) -> Result<Option<TunnelCredentials>> {
    match status {
        ERROR_CANCELLED => Ok(None),
        ERROR_SUCCESS => unpack(output).map(Some),
        _ => Err(SecretError::Unavailable),
    }
}

fn unpack(buffer: &AuthBuffer) -> Result<TunnelCredentials> {
    if buffer.pointer.is_null() || buffer.length == 0 || buffer.length > 65_536 {
        return Err(SecretError::Invalid);
    }
    // Fixed capacities include NUL. Oversize fails instead of truncating a key.
    let mut user = Zeroizing::new(vec![0u16; 129]);
    let mut domain = Zeroizing::new(vec![0u16; 129]);
    let mut password = Zeroizing::new(vec![0u16; 2049]);
    let (mut user_len, mut domain_len, mut password_len) = (
        user.len() as u32,
        domain.len() as u32,
        password.len() as u32,
    );
    // SAFETY: OS-owned blob and writable slices remain alive throughout the
    // call; counts describe allocated UTF-16 capacities, not bytes.
    if unsafe {
        CredUnPackAuthenticationBufferW(
            0,
            buffer.pointer,
            buffer.length,
            user.as_mut_ptr(),
            &mut user_len,
            domain.as_mut_ptr(),
            &mut domain_len,
            password.as_mut_ptr(),
            &mut password_len,
        )
    } == 0
    {
        return Err(SecretError::Invalid);
    }
    let user = ascii_z(&user)?;
    let domain = ascii_z(&domain)?;
    let password = ascii_z(&password)?;
    if !domain.is_empty() || password.len() >= NATIVE_KEY_BOUNDARY {
        return Err(SecretError::Invalid);
    }
    let id = String::from_utf8(user.to_vec()).map_err(|_| SecretError::Invalid)?;
    TunnelCredentials::new(id, password.to_vec())
}
fn ascii_z(value: &[u16]) -> Result<Zeroizing<Vec<u8>>> {
    let end = value
        .iter()
        .position(|&c| c == 0)
        .ok_or(SecretError::Invalid)?;
    if value[..end].iter().any(|&c| !(0x21..=0x7e).contains(&c)) {
        return Err(SecretError::Invalid);
    }
    Ok(Zeroizing::new(
        value[..end].iter().map(|&c| c as u8).collect(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows_sys::Win32::{
        Security::Credentials::{CRED_PACK_GENERIC_CREDENTIALS, CredPackAuthenticationBufferW},
        System::Com::CoTaskMemAlloc,
    };
    fn packed(user: &str, password: &str) -> AuthBuffer {
        let user: Vec<_> = user.encode_utf16().chain(Some(0)).collect();
        let password = Zeroizing::new(password.encode_utf16().chain(Some(0)).collect::<Vec<_>>());
        let mut length = 0;
        unsafe {
            CredPackAuthenticationBufferW(
                CRED_PACK_GENERIC_CREDENTIALS,
                user.as_ptr(),
                password.as_ptr(),
                ptr::null_mut(),
                &mut length,
            );
        }
        assert!(
            length > 0 && length <= 65_536,
            "native pack size failed: user units={}, password units={}, OS error={}",
            user.len(),
            password.len(),
            unsafe { windows_sys::Win32::Foundation::GetLastError() }
        );
        let buffer = AuthBuffer {
            pointer: unsafe { CoTaskMemAlloc(length as usize) },
            length,
        };
        assert!(!buffer.pointer.is_null());
        assert_ne!(
            unsafe {
                CredPackAuthenticationBufferW(
                    CRED_PACK_GENERIC_CREDENTIALS,
                    user.as_ptr(),
                    password.as_ptr(),
                    buffer.pointer.cast(),
                    &mut length,
                )
            },
            0
        );
        buffer
    }
    #[test]
    fn native_blob_unpack_preserves_key_and_rejects_invalid_or_oversized_input() {
        let id = "tunnel_0123456789abcdef0123456789abcdef";
        for key in ["synthetic-input-key-12345".to_owned(), "a".repeat(255)] {
            let c = unpack(&packed(id, &key)).unwrap();
            assert_eq!(c.tunnel_id(), id);
            assert_eq!(c.key_for_tunnel(), key.as_bytes());
        }
        for (user, key) in [
            (id.to_owned(), "a".repeat(256)),
            ("tunnel_".to_owned() + &"a".repeat(122), "a".repeat(32)),
            ("DOMAIN\\user".to_owned(), "a".repeat(32)),
            (id.to_owned(), "short".to_owned()),
            (id.to_owned(), "synthetic key has spaces".to_owned()),
            (id.to_owned(), "synthetic-秘密-key-12345".to_owned()),
        ] {
            assert!(matches!(
                unpack(&packed(&user, &key)),
                Err(SecretError::Invalid)
            ));
        }
        assert!(ascii_z(&[b'a' as u16; 10]).is_err());
    }
    #[test]
    fn cancellation_and_native_errors_do_not_produce_credentials() {
        let empty = AuthBuffer {
            pointer: ptr::null_mut(),
            length: 0,
        };
        assert!(finish_prompt(ERROR_CANCELLED, &empty).unwrap().is_none());
        assert!(matches!(
            finish_prompt(5, &empty),
            Err(SecretError::Unavailable)
        ));
        assert!(matches!(
            finish_prompt(ERROR_SUCCESS, &empty),
            Err(SecretError::Invalid)
        ));
    }
}
