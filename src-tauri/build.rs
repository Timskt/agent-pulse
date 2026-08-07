fn main() {
    println!("cargo:rerun-if-changed=windows-test.rc");
    println!("cargo:rerun-if-changed=windows-test.manifest");

    let windows = tauri_build::WindowsAttributes::new_without_app_manifest();
    tauri_build::try_build(tauri_build::Attributes::new().windows_attributes(windows))
        .expect("failed to run Tauri build script");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        // tauri-winres normally emits the app manifest through
        // `rustc-link-arg-bins`, which excludes Cargo's lib unit-test harness.
        // We disable only that copy above, then link the same Tauri manifest into
        // every final PE artifact. This keeps the shipped app unchanged while
        // ensuring comctl32!TaskDialogIndirect resolves under Common Controls v6
        // before the Rust harness starts.
        embed_resource::compile_for_everything("windows-test.rc", embed_resource::NONE)
            .manifest_required()
            .expect("every Windows PE artifact requires the Common Controls v6 manifest");
    }
}
