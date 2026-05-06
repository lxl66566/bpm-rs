fn main() {
    let target = std::env::var("TARGET").unwrap();
    if !target.contains("windows") {
        return;
    }

    let out_dir = std::env::var("OUT_DIR").unwrap();
    let exe_path = format!("{out_dir}/bpm-shim.exe");
    let zst_path = format!("{out_dir}/bpm-shim.exe.zst");

    let status = std::process::Command::new("rustc")
        .args([
            "shim.rs",
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
