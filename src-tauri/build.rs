fn main() {
    let attributes = tauri_build::Attributes::new()
        .windows_attributes(tauri_build::WindowsAttributes::new_without_app_manifest());
    tauri_build::try_build(attributes).expect("failed to prepare Tauri build metadata");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        // Tauri normally links its manifest only to app binaries. Linking this shared
        // manifest to every artifact gives both the app and Rust test harnesses the
        // Common Controls v6 activation context required by TaskDialogIndirect.
        embed_resource::compile_for_everything("windows-app-manifest.rc", embed_resource::NONE)
            .manifest_required()
            .expect("failed to embed the Windows Common Controls v6 manifest");
    }
}
