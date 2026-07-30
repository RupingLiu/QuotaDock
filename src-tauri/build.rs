fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        ensure_webview2_loader_resource();
    }

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

fn ensure_webview2_loader_resource() {
    let destination = std::path::Path::new("resources")
        .join("windows")
        .join("WebView2Loader.dll");
    if destination.is_file() {
        return;
    }

    let architecture = match std::env::var("CARGO_CFG_TARGET_ARCH").as_deref() {
        Ok("x86_64") => "x64",
        Ok("x86") => "x86",
        Ok("aarch64") => "arm64",
        Ok(other) => panic!("unsupported Windows architecture for WebView2Loader.dll: {other}"),
        Err(error) => panic!("missing CARGO_CFG_TARGET_ARCH: {error}"),
    };
    let cargo_home = std::env::var_os("CARGO_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::var_os("USERPROFILE")
                .map(std::path::PathBuf::from)
                .map(|profile| profile.join(".cargo"))
        })
        .expect("cannot locate Cargo home for WebView2Loader.dll");
    let registry_sources = cargo_home.join("registry").join("src");
    let mut candidates = Vec::new();
    for registry in std::fs::read_dir(&registry_sources)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", registry_sources.display()))
        .filter_map(Result::ok)
    {
        for package in std::fs::read_dir(registry.path())
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
        {
            if !package
                .file_name()
                .to_string_lossy()
                .starts_with("webview2-com-sys-")
            {
                continue;
            }
            let candidate = package.path().join(architecture).join("WebView2Loader.dll");
            if candidate.is_file() {
                candidates.push(candidate);
            }
        }
    }
    candidates.sort();
    let source = candidates
        .pop()
        .expect("WebView2Loader.dll is missing from the downloaded webview2-com-sys crate");
    let parent = destination
        .parent()
        .expect("WebView2Loader.dll destination has no parent");
    std::fs::create_dir_all(parent)
        .unwrap_or_else(|error| panic!("cannot create {}: {error}", parent.display()));
    std::fs::copy(&source, &destination).unwrap_or_else(|error| {
        panic!(
            "cannot copy {} to {}: {error}",
            source.display(),
            destination.display()
        )
    });
    println!("cargo:rerun-if-changed={}", source.display());
}
