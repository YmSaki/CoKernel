//! All impersonation is synchronous and ends before returning to async code.
use std::{
    io,
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
    ptr,
};
use tokio::net::windows::named_pipe::{NamedPipeServer, ServerOptions};
use windows_sys::Win32::{
    Foundation::{HANDLE, LocalFree},
    Security::{
        Authorization::{
            ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
        },
        GetTokenInformation, RevertToSelf, SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER, TokenUser,
    },
    System::{
        Pipes::ImpersonateNamedPipeClient,
        Threading::{GetCurrentProcess, GetCurrentThread, OpenProcessToken, OpenThreadToken},
    },
};

fn token_sid(token: HANDLE) -> io::Result<String> {
    let mut needed = 0;
    // SAFETY: this first call queries the buffer size and writes only needed.
    unsafe {
        GetTokenInformation(token, TokenUser, ptr::null_mut(), 0, &mut needed);
    }
    if needed == 0 || needed > 65536 {
        return Err(io::Error::other("invalid Windows token size"));
    }
    let mut buffer = vec![0usize; (needed as usize).div_ceil(std::mem::size_of::<usize>())];
    // SAFETY: buffer has pointer alignment and at least needed writable bytes.
    if unsafe {
        GetTokenInformation(
            token,
            TokenUser,
            buffer.as_mut_ptr().cast(),
            needed,
            &mut needed,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: a successful TokenUser query returned an aligned TOKEN_USER.
    let user = unsafe { &*buffer.as_ptr().cast::<TOKEN_USER>() };
    let mut value = ptr::null_mut();
    // SAFETY: User.Sid is owned by the live token query buffer; output uses LocalAlloc.
    if unsafe { ConvertSidToStringSidW(user.User.Sid, &mut value) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let allocation = LocalAllocation(value.cast());
    let mut length = 0;
    // Windows SID strings have a bounded canonical representation.
    while length < 256 && unsafe { *value.add(length) } != 0 {
        length += 1;
    }
    if length == 256 {
        return Err(io::Error::other("unexpected SID string size"));
    }
    // SAFETY: ConvertSidToStringSidW returns a NUL-terminated UTF-16 allocation.
    let result = String::from_utf16(unsafe { std::slice::from_raw_parts(value, length) })
        .map_err(io::Error::other);
    drop(allocation);
    result
}
struct LocalAllocation(*mut std::ffi::c_void);
impl Drop for LocalAllocation {
    fn drop(&mut self) {
        unsafe {
            LocalFree(self.0);
        }
    }
}
pub fn current_sid() -> io::Result<String> {
    let mut token = ptr::null_mut();
    // SAFETY: pseudo process handle is valid; token output becomes owned below.
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let owned = unsafe { OwnedHandle::from_raw_handle(token) };
    token_sid(owned.as_raw_handle())
}

pub fn authorize_server(pipe: &tokio::net::windows::named_pipe::NamedPipeClient) -> io::Result<()> {
    use windows_sys::Win32::System::{
        Pipes::GetNamedPipeServerProcessId,
        Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION},
    };
    let mut pid = 0;
    // SAFETY: connected pipe handle is valid; output points to a live u32.
    if unsafe { GetNamedPipeServerProcessId(pipe.as_raw_handle(), &mut pid) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if process.is_null() {
        return Err(io::Error::last_os_error());
    }
    let process = unsafe { OwnedHandle::from_raw_handle(process) };
    let mut token = ptr::null_mut();
    if unsafe { OpenProcessToken(process.as_raw_handle(), TOKEN_QUERY, &mut token) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let token = unsafe { OwnedHandle::from_raw_handle(token) };
    if token_sid(token.as_raw_handle())? != current_sid()? {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Host belongs to another Windows user",
        ));
    }
    Ok(())
}

fn descriptor(sid: &str) -> io::Result<LocalAllocation> {
    // SID is obtained from Windows, not client input. Tests also use a known SID.
    if !sid.starts_with("S-")
        || !sid
            .bytes()
            .all(|b| b.is_ascii_digit() || b == b'-' || b == b'S')
    {
        return Err(io::Error::other("invalid SID"));
    }
    let text: Vec<u16> = format!("D:P(A;;GA;;;{sid})")
        .encode_utf16()
        .chain(Some(0))
        .collect();
    let mut pointer = ptr::null_mut();
    // SAFETY: input is terminated and output is LocalAlloc-owned.
    if unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            text.as_ptr(),
            1,
            &mut pointer,
            ptr::null_mut(),
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(LocalAllocation(pointer))
}
pub fn create(name: &str, sid: &str, first: bool) -> io::Result<NamedPipeServer> {
    let descriptor = descriptor(sid)?;
    let mut attributes = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor.0,
        bInheritHandle: 0,
    };
    // SAFETY: descriptor/attributes stay alive until CreateNamedPipe returns;
    // the kernel captures the security descriptor. Pipe handle is non-inheritable.
    unsafe {
        ServerOptions::new()
            .first_pipe_instance(first)
            .reject_remote_clients(true)
            .max_instances(33)
            .in_buffer_size(65536)
            .out_buffer_size(65536)
            .create_with_security_attributes_raw(
                name,
                (&mut attributes as *mut SECURITY_ATTRIBUTES).cast(),
            )
    }
}
pub fn authorize(pipe: &NamedPipeServer, expected: &str) -> io::Result<()> {
    // Must be called after reading the initial frame and must never await.
    if unsafe { ImpersonateNamedPipeClient(pipe.as_raw_handle()) } == 0 {
        return Err(io::Error::last_os_error());
    }
    struct Revert;
    impl Drop for Revert {
        fn drop(&mut self) {
            // Continuing a Tokio thread with a client identity is unsafe. Microsoft
            // explicitly requires terminating the process when RevertToSelf fails.
            if unsafe { RevertToSelf() } == 0 {
                std::process::abort();
            }
        }
    }
    let revert = Revert;
    let mut token = ptr::null_mut();
    let result = if unsafe { OpenThreadToken(GetCurrentThread(), TOKEN_QUERY, 1, &mut token) } == 0
    {
        Err(io::Error::last_os_error())
    } else {
        let token = unsafe { OwnedHandle::from_raw_handle(token) };
        token_sid(token.as_raw_handle()).and_then(|sid| {
            if sid == expected {
                Ok(())
            } else {
                Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "different Windows user",
                ))
            }
        })
    };
    drop(revert);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn kernel_acl_denies_a_client_not_named_in_the_dacl() {
        let name = format!(r"\\.\pipe\cokernel-v1-acl-{}", uuid::Uuid::new_v4());
        assert_ne!(current_sid().unwrap(), "S-1-5-18");
        let _server = create(&name, "S-1-5-18", true).unwrap();
        let error = tokio::net::windows::named_pipe::ClientOptions::new()
            .open(&name)
            .unwrap_err();
        assert_eq!(error.raw_os_error(), Some(5));
    }
    #[tokio::test]
    async fn peer_identity_mismatch_is_rejected_and_thread_identity_is_restored() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let name = format!(r"\\.\pipe\cokernel-v1-identity-{}", uuid::Uuid::new_v4());
        let sid = current_sid().unwrap();
        let mut server = create(&name, &sid, true).unwrap();
        let check = tokio::spawn(async move {
            server.connect().await.unwrap();
            server.read_u8().await.unwrap();
            assert_eq!(
                authorize(&server, "S-1-5-18").unwrap_err().kind(),
                io::ErrorKind::PermissionDenied
            );
            let mut token = ptr::null_mut();
            // Check the thread token, not just the unchanged process token.
            assert_eq!(
                unsafe { OpenThreadToken(GetCurrentThread(), TOKEN_QUERY, 1, &mut token) },
                0
            );
            assert_eq!(io::Error::last_os_error().raw_os_error(), Some(1008)); // ERROR_NO_TOKEN
            assert_eq!(current_sid().unwrap(), sid);
        });
        let mut client = tokio::net::windows::named_pipe::ClientOptions::new()
            .open(&name)
            .unwrap();
        client.write_u8(1).await.unwrap();
        check.await.unwrap();
    }
    #[test]
    fn descriptor_has_one_protected_allow_ace_for_current_user_only() {
        use windows_sys::Win32::Security::{
            ACCESS_ALLOWED_ACE, GetAce, GetSecurityDescriptorControl, GetSecurityDescriptorDacl,
            SE_DACL_PROTECTED,
        };
        let sid = current_sid().unwrap();
        let descriptor = descriptor(&sid).unwrap();
        let mut present = 0;
        let mut defaulted = 0;
        let mut acl = ptr::null_mut();
        let mut control = 0;
        let mut revision = 0;
        unsafe {
            assert_ne!(
                GetSecurityDescriptorDacl(descriptor.0, &mut present, &mut acl, &mut defaulted),
                0
            );
            assert_eq!(present, 1);
            assert!(!acl.is_null());
            assert_eq!((*acl).AceCount, 1);
            assert_ne!(
                GetSecurityDescriptorControl(descriptor.0, &mut control, &mut revision),
                0
            );
            assert_ne!(control & SE_DACL_PROTECTED, 0);
            let mut ace = ptr::null_mut();
            assert_ne!(GetAce(acl, 0, &mut ace), 0);
            let ace = &*ace.cast::<ACCESS_ALLOWED_ACE>();
            assert_eq!(ace.Header.AceType, 0);
            let mut value = ptr::null_mut();
            assert_ne!(
                ConvertSidToStringSidW((&ace.SidStart as *const u32).cast_mut().cast(), &mut value),
                0
            );
            let allocation = LocalAllocation(value.cast());
            let mut length = 0;
            while *value.add(length) != 0 {
                length += 1;
            }
            assert_eq!(
                String::from_utf16(std::slice::from_raw_parts(value, length)).unwrap(),
                sid
            );
            drop(allocation);
        }
    }
}
