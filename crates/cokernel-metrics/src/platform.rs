#[cfg(target_os = "linux")]
#[path = "linux.rs"]
mod implementation;
#[cfg(windows)]
#[path = "windows.rs"]
mod implementation;
#[cfg(any(windows, target_os = "linux"))]
pub(super) use implementation::*;
