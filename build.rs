fn main() {
    // Cargo always sets TARGET when running build scripts; fail loudly if not.
    let target = std::env::var("TARGET").expect("cargo did not set TARGET");
    if !target.contains("windows") {
        return;
    }

    // OUT_DIR is already target-scoped (target/<triple>/debug/build/...),
    // so the artifacts of different targets never collide.
    let out_dir = std::env::var("OUT_DIR").unwrap();
    let exe_path = format!("{out_dir}/bpm-shim.exe");
    let zst_path = format!("{out_dir}/bpm-shim.exe.zst");

    // Compile for the cargo target, not the host: without --target, rustc
    // builds a host binary, which would embed a wrong-platform shim when
    // cross-compiling. Requires the target's rust-std, same as the main
    // crate, so no extra prerequisite in practice.
    let status = std::process::Command::new("rustc")
        .args([
            "shim.rs",
            "--target",
            &target,
            "-o",
            &exe_path,
            "-C",
            "opt-level=s",
            "-C",
            "panic=abort",
            "-C",
            "strip=symbols",
        ])
        .status()
        .expect("failed to invoke rustc for shim");
    assert!(status.success(), "rustc failed for shim");

    let raw = std::fs::read(&exe_path).expect("failed to read compiled shim");
    let compressed = zstd::bulk::compress(&raw, 19).expect("failed to zstd-compress shim");
    std::fs::write(&zst_path, compressed).expect("failed to write compressed shim");

    println!("cargo:rerun-if-changed=shim.rs");
}
