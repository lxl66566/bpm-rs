use std::{
    fs::File,
    io::Write,
    path::{Path, PathBuf},
};

use bpm::storage::db::DbOperation;

#[allow(dead_code)]
pub struct TestEnv {
    _guard: tempfile::TempDir,
    install_pos: PathBuf,
    db_path: PathBuf,
    app_path: PathBuf,
    bin_path: PathBuf,
}

#[allow(dead_code)]
impl TestEnv {
    pub fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let install_pos = dir.path().join("bpm");
        Self {
            app_path: install_pos.join("app"),
            bin_path: install_pos.join("bin"),
            db_path: dir.path().join("db.ron"),
            install_pos,
            _guard: dir,
        }
    }

    pub fn ctx(&self) -> bpm::context::Context {
        bpm::context::Context::new()
            .with_install_position(&self.install_pos)
            .with_db_path(&self.db_path)
    }

    pub fn app_path(&self) -> &Path {
        &self.app_path
    }

    pub fn bin_path(&self) -> &Path {
        &self.bin_path
    }

    pub fn db_path(&self) -> &Path {
        &self.db_path
    }

    pub fn tmp(&self) -> &Path {
        self._guard.path()
    }

    pub fn db(&self) -> bpm::storage::db::Db {
        bpm::storage::db::Db::create_or_open(self.db_path()).unwrap()
    }
}

pub fn create_test_zip(zip_path: &Path, files: &[(&str, &[u8])]) {
    let file = File::create(zip_path).unwrap();
    let mut zip = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default();
    for (name, content) in files {
        zip.start_file(name, options).unwrap();
        zip.write_all(content).unwrap();
    }
    zip.finish().unwrap();
}
