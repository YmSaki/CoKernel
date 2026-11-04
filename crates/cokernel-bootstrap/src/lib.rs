//! Windows/WSL bootstrap and installer support for CoKernel v1.
//!
//! Dedicated validation image/import primitives precede #34 installer UX.
//! Destructive legacy reset remains behind explicit authorization and tests.

pub const PRODUCT_DISTRO_NAME: &str = "CoKernel";

pub mod image;
#[cfg(target_os = "linux")]
pub mod provision;
#[cfg(windows)]
pub mod windows;
