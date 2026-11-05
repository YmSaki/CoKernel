fn main() {
    #[cfg(windows)]
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(
        tauri_build::AppManifest::new().commands(&[
            "host_call",
            "host_observation",
            "configure_remote",
        ]),
    ))
    .expect("Tauri build configuration must be valid");
}
