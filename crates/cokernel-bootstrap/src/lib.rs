//! Windows/WSL bootstrap and installer support for CoKernel v1.
//!
//! Real implementation is tracked by #34. Keep destructive legacy-reset logic
//! behind explicit authorization and tests; do not place it in ad-hoc scripts.

pub const PRODUCT_DISTRO_NAME: &str = "CoKernelV1";
