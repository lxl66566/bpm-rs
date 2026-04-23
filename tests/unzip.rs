use std::fs;

use bpm::installation::unzip::unzip;

fn assets_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test_assets")
}

#[test]
fn unzip_noroot_zip() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("noroot.zip");
    fs::copy(assets_dir().join("noroot.zip"), &src).unwrap();

    let out = tempfile::tempdir().unwrap();
    let main = unzip(&src, out.path()).unwrap();
    assert_eq!(main, out.path());
    assert!(out.path().is_dir());
    assert!(!src.exists(), "archive should be removed after extraction");
}

#[test]
fn unzip_noroot_tar_gz() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("noroot.tar.gz");
    fs::copy(assets_dir().join("noroot.tar.gz"), &src).unwrap();

    let out = tempfile::tempdir().unwrap();
    let main = unzip(&src, out.path()).unwrap();
    assert_eq!(main, out.path());
    assert!(!src.exists());
}

#[test]
fn unzip_root_tar_gz_unwraps_single_dir() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("root.tar.gz");
    fs::copy(assets_dir().join("root.tar.gz"), &src).unwrap();

    let out = tempfile::tempdir().unwrap();
    let main = unzip(&src, out.path()).unwrap();
    assert_eq!(main, out.path().join("root"));
    assert!(main.is_dir());
    assert!(!src.exists());
}
