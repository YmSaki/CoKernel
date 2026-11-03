//! Current-user sign-in registration. Never changes Runtime desired state.
use crate::bridge::BridgeError;
use serde::Serialize;
use std::{
    path::{Path, PathBuf},
    ptr,
};
use windows_sys::Win32::{
    Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS},
    System::Registry::*,
};

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const VALUE: &str = "CoKernelHost";
const MAX_COMMAND_UNITS: usize = 260;
type Result<T> = std::result::Result<T, BridgeError>;
fn error(message: &str) -> BridgeError {
    BridgeError::new("CK-HOST-AUTOSTART", message)
}
fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}

#[derive(Clone)]
pub struct AutostartTarget {
    executable: PathBuf,
    config: PathBuf,
    state: PathBuf,
}
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct AutostartStatus {
    pub registered: bool,
    pub matches_current_launch: bool,
    /// Windows policy/Startup Apps can independently suppress registered entries.
    pub effective_enabled: Option<bool>,
}
impl AutostartTarget {
    pub fn new(executable: PathBuf, config: PathBuf, state: PathBuf) -> Result<Self> {
        for path in [&executable, &config, &state] {
            let value = path
                .to_str()
                .ok_or_else(|| error("Startup paths must be valid Unicode"))?;
            if !path.is_absolute() || value.chars().any(|c| c.is_control() || c == '"') {
                return Err(error(
                    "Startup paths must be absolute and contain no quote or control characters",
                ));
            }
        }
        Ok(Self {
            executable,
            config,
            state,
        })
    }
    fn command(&self) -> Result<String> {
        // All fields are validated paths; no shell or client-supplied command is used.
        // Double trailing backslashes before the quote for Windows argv parsing.
        fn quote(path: &Path) -> String {
            let value = path.to_string_lossy();
            let trailing = value.chars().rev().take_while(|c| *c == '\\').count();
            format!("\"{value}{}\"", "\\".repeat(trailing))
        }
        let command = format!(
            "{} serve {} {} --background",
            quote(&self.executable),
            quote(&self.config),
            quote(&self.state)
        );
        if command.encode_utf16().count() > MAX_COMMAND_UNITS {
            return Err(error(
                "Startup command exceeds Windows Run's 260-character limit; use shorter installation/configuration paths",
            ));
        }
        Ok(command)
    }
    pub fn status(&self) -> Result<AutostartStatus> {
        self.status_at(RUN_KEY)
    }
    pub fn set_enabled(&self, enabled: bool) -> Result<AutostartStatus> {
        self.set_at(RUN_KEY, enabled)
    }
    fn status_at(&self, key: &str) -> Result<AutostartStatus> {
        let value = read_value(key, VALUE)?;
        Ok(AutostartStatus {
            registered: value.is_some(),
            matches_current_launch: value
                .as_ref()
                .is_some_and(|v| self.command().is_ok_and(|expected| *v == expected)),
            effective_enabled: if value.is_none() { Some(false) } else { None },
        })
    }
    fn set_at(&self, key: &str, enabled: bool) -> Result<AutostartStatus> {
        if enabled {
            if !self.executable.is_file() || !self.config.is_file() {
                return Err(error(
                    "Host executable and WSL configuration must exist before enabling startup",
                ));
            }
            use std::io::Read;
            let mut bytes = Vec::new();
            std::fs::File::open(&self.config)
                .and_then(|f| f.take(16_385).read_to_end(&mut bytes))
                .map_err(|_| error("Cannot read WSL configuration"))?;
            if bytes.len() > 16_384 {
                return Err(error("WSL configuration exceeds its size limit"));
            }
            let config: crate::bridge::WslConfig =
                serde_json::from_slice(&bytes).map_err(|_| error("Invalid WSL configuration"))?;
            config.command()?;
            let command = self.command()?;
            let handle =
                Key::open(key, true)?.ok_or_else(|| error("Cannot create startup registry key"))?;
            let value = wide(&command);
            let name = wide(VALUE);
            // SAFETY: handle is live; both buffers remain valid for this synchronous call.
            let result = unsafe {
                RegSetValueExW(
                    handle.0,
                    name.as_ptr(),
                    0,
                    REG_SZ,
                    value.as_ptr().cast(),
                    (value.len() * 2) as u32,
                )
            };
            if result != ERROR_SUCCESS {
                return Err(error("Cannot write current-user startup registration"));
            }
        } else if let Some(handle) = Key::open(key, false)? {
            let name = wide(VALUE);
            // SAFETY: handle/name are live; only our fixed product value is removed.
            let result = unsafe { RegDeleteValueW(handle.0, name.as_ptr()) };
            if result != ERROR_SUCCESS && result != ERROR_FILE_NOT_FOUND {
                return Err(error("Cannot remove current-user startup registration"));
            }
        }
        self.status_at(key)
    }
}
pub(crate) async fn dispatch(
    host: &crate::supervisor::Host,
    id: String,
    method: &str,
    params: serde_json::Value,
) -> cokernel_protocol::ResponseEnvelope {
    let enabled = if method == "host.autostart.set" {
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Params {
            enabled: bool,
        }
        match serde_json::from_value::<Params>(params) {
            Ok(p) => Some(p.enabled),
            Err(_) => {
                return crate::ipc::error(
                    id,
                    "CK-HOST-REQUEST",
                    "Autostart accepts only a boolean enabled setting",
                );
            }
        }
    } else {
        if params != serde_json::json!({}) {
            return crate::ipc::error(
                id,
                "CK-HOST-REQUEST",
                "Autostart status takes empty parameters",
            );
        }
        None
    };
    let Some(target) = host.autostart_target() else {
        return crate::ipc::error(
            id,
            "CK-HOST-AUTOSTART",
            "Host has no trusted startup configuration",
        );
    };
    let result = tokio::task::spawn_blocking(move || match enabled {
        Some(value) => target.set_enabled(value),
        None => target.status(),
    })
    .await;
    match result {
        Ok(Ok(status)) => crate::ipc::success(id, serde_json::json!(status)),
        Ok(Err(e)) => crate::ipc::error(id, e.code, &e.message),
        Err(_) => crate::ipc::error(id, "CK-HOST-AUTOSTART", "Startup setting task failed"),
    }
}
struct Key(HKEY);
impl Key {
    fn open(path: &str, create: bool) -> Result<Option<Self>> {
        let path = wide(path);
        let mut handle = ptr::null_mut();
        // SAFETY: UTF-16 path and output handle are valid; scope is always HKCU.
        let result = unsafe {
            if create {
                RegCreateKeyExW(
                    HKEY_CURRENT_USER,
                    path.as_ptr(),
                    0,
                    ptr::null(),
                    REG_OPTION_NON_VOLATILE,
                    KEY_SET_VALUE,
                    ptr::null(),
                    &mut handle,
                    ptr::null_mut(),
                )
            } else {
                RegOpenKeyExW(
                    HKEY_CURRENT_USER,
                    path.as_ptr(),
                    0,
                    KEY_SET_VALUE,
                    &mut handle,
                )
            }
        };
        if result == ERROR_FILE_NOT_FOUND {
            return Ok(None);
        }
        if result != ERROR_SUCCESS {
            return Err(error("Cannot open current-user startup registration"));
        }
        Ok(Some(Self(handle)))
    }
}
impl Drop for Key {
    fn drop(&mut self) {
        unsafe {
            RegCloseKey(self.0);
        }
    }
}
fn read_value(key: &str, name: &str) -> Result<Option<String>> {
    let key = wide(key);
    let name = wide(name);
    let mut buffer = [0u16; MAX_COMMAND_UNITS + 1];
    let mut size = (buffer.len() * 2) as u32;
    // SAFETY: fixed-size aligned buffer and byte count match; RegGetValue adds/checks the terminator.
    let result = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            name.as_ptr(),
            RRF_RT_REG_SZ,
            ptr::null_mut(),
            buffer.as_mut_ptr().cast(),
            &mut size,
        )
    };
    if result == ERROR_FILE_NOT_FOUND {
        return Ok(None);
    }
    if result != ERROR_SUCCESS
        || size < 2
        || size as usize > buffer.len() * 2
        || !size.is_multiple_of(2)
    {
        return Err(error(
            "Startup registration is unreadable, oversized or not a string",
        ));
    }
    let units = size as usize / 2;
    if buffer[units - 1] != 0 || buffer[..units - 1].contains(&0) {
        return Err(error("Startup registration has invalid string data"));
    }
    String::from_utf16(&buffer[..units - 1])
        .map(Some)
        .map_err(|_| error("Startup registration is not valid Unicode"))
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Fixture {
        root: PathBuf,
        key: String,
    }
    impl Fixture {
        fn new() -> Self {
            let id = uuid::Uuid::new_v4();
            let root =
                std::env::temp_dir().join(format!("ckas-{}", &id.simple().to_string()[..12]));
            std::fs::create_dir_all(&root).unwrap();
            Self {
                root,
                key: format!(r"Software\CoKernel\Tests\{id}"),
            }
        }
        fn target(&self) -> AutostartTarget {
            let executable = self.root.join("host 日本.exe");
            let config = self.root.join("wsl & config.json");
            std::fs::write(&executable, []).unwrap();
            std::fs::write(&config,br#"{"distribution":"unused","linux_user":"runtime","runtime_path":"/opt/cokernel/runtime"}"#).unwrap();
            AutostartTarget::new(executable, config, self.root.join("state")).unwrap()
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let key = wide(&self.key);
            // Only the unique disposable test key, never the Windows Run key.
            unsafe {
                RegDeleteTreeW(HKEY_CURRENT_USER, key.as_ptr());
            }
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }
    #[test]
    fn registry_round_trip_is_scoped_and_leaves_desired_state_untouched() {
        let f = Fixture::new();
        let target = f.target();
        let (store, _) = crate::state::StateStore::open(&target.state).unwrap();
        store.save(crate::state::DesiredState::Stopped).unwrap();
        drop(store);
        let before = std::fs::read(target.state.join("desired.json")).unwrap();
        assert!(!target.status_at(&f.key).unwrap().registered);
        let registered = target.set_at(&f.key, true).unwrap();
        assert!(registered.registered && registered.matches_current_launch);
        assert_eq!(registered.effective_enabled, None);
        assert_eq!(target.set_at(&f.key, true).unwrap(), registered);
        let text = read_value(&f.key, VALUE).unwrap().unwrap();
        assert!(text.ends_with("--background") && !text.contains("--start"));
        assert_eq!(
            target.set_at(&f.key, false).unwrap().effective_enabled,
            Some(false)
        );
        assert!(!target.set_at(&f.key, false).unwrap().registered);
        assert_eq!(
            std::fs::read(target.state.join("desired.json")).unwrap(),
            before
        );
    }
    #[test]
    fn quoted_command_round_trips_through_windows_argument_parser() {
        let target = AutostartTarget::new(
            "C:\\Program Files\\日本\\host.exe".into(),
            "C:\\settings & data\\wsl.json".into(),
            "C:\\state with spaces\\".into(),
        )
        .unwrap();
        let command = wide(&target.command().unwrap());
        let mut count = 0;
        // The allocation returned by Windows contains `count` terminated strings.
        unsafe {
            let args =
                windows_sys::Win32::UI::Shell::CommandLineToArgvW(command.as_ptr(), &mut count);
            assert!(!args.is_null());
            let parsed: Vec<String> = std::slice::from_raw_parts(args, count as usize)
                .iter()
                .map(|&arg| {
                    let mut len = 0;
                    while *arg.add(len) != 0 {
                        len += 1;
                    }
                    String::from_utf16(std::slice::from_raw_parts(arg, len)).unwrap()
                })
                .collect();
            windows_sys::Win32::Foundation::LocalFree(args.cast());
            assert_eq!(
                parsed,
                [
                    target.executable.to_str().unwrap(),
                    "serve",
                    target.config.to_str().unwrap(),
                    target.state.to_str().unwrap(),
                    "--background"
                ]
            );
        }
    }
    #[test]
    fn foreign_values_and_malformed_registration_are_not_silently_accepted() {
        let f = Fixture::new();
        let target = f.target();
        let key = Key::open(&f.key, true).unwrap().unwrap();
        let neighbor = wide("AnotherProduct");
        let value = wide("preserve");
        unsafe {
            assert_eq!(
                RegSetValueExW(
                    key.0,
                    neighbor.as_ptr(),
                    0,
                    REG_SZ,
                    value.as_ptr().cast(),
                    (value.len() * 2) as u32
                ),
                ERROR_SUCCESS
            );
        }
        target.set_at(&f.key, true).unwrap();
        let other = AutostartTarget::new(
            target.executable.clone(),
            target.config.clone(),
            f.root.join("other-state"),
        )
        .unwrap();
        assert!(!other.status_at(&f.key).unwrap().matches_current_launch);
        let name = wide(VALUE);
        let oversized = wide(&"x".repeat(261));
        unsafe {
            assert_eq!(
                RegSetValueExW(
                    key.0,
                    name.as_ptr(),
                    0,
                    REG_SZ,
                    oversized.as_ptr().cast(),
                    (oversized.len() * 2) as u32
                ),
                ERROR_SUCCESS
            );
        }
        assert!(target.status_at(&f.key).is_err());
        let number = 1u32;
        unsafe {
            assert_eq!(
                RegSetValueExW(
                    key.0,
                    name.as_ptr(),
                    0,
                    REG_DWORD,
                    (&number as *const u32).cast(),
                    4
                ),
                ERROR_SUCCESS
            );
        }
        assert!(target.status_at(&f.key).is_err());
        assert!(!target.set_at(&f.key, false).unwrap().registered);
        assert_eq!(
            read_value(&f.key, "AnotherProduct").unwrap().as_deref(),
            Some("preserve")
        );
    }
    #[test]
    fn invalid_and_overlong_launches_do_not_register() {
        let f = Fixture::new();
        let target = f.target();
        assert!(
            AutostartTarget::new(
                "relative.exe".into(),
                target.config.clone(),
                target.state.clone()
            )
            .is_err()
        );
        assert!(
            AutostartTarget::new(
                "C:\\bad\".exe".into(),
                target.config.clone(),
                target.state.clone()
            )
            .is_err()
        );
        let long = AutostartTarget::new(
            target.executable,
            target.config,
            PathBuf::from(format!("C:\\{}", "x".repeat(260))),
        )
        .unwrap();
        assert!(long.set_at(&f.key, true).is_err());
        assert!(read_value(&f.key, VALUE).unwrap().is_none());
    }
}
