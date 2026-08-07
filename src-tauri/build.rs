fn main() {
    tauri_build::build();

    println!("cargo:rerun-if-changed=windows-test.rc");
    println!("cargo:rerun-if-changed=windows-test.manifest");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        // tauri-build/tauri-winres uses `rustc-link-arg-bins`, so its manifest is
        // present in the shipped app but absent from Cargo's lib test harness.
        // Tauri's common-controls-v6 feature statically references
        // comctl32!TaskDialogIndirect; without this test-only resource Windows can
        // reject the EXE in the loader with STATUS_ENTRYPOINT_NOT_FOUND before the
        // Rust harness gets a chance to print a single test name.
        embed_resource::compile_for_tests("windows-test.rc", embed_resource::NONE)
            .manifest_required()
            .expect("Windows test harness requires the Common Controls v6 manifest");
    }
}
