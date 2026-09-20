//! bpm shim executable — acts as a transparent proxy to the real executable.
//!
//! This binary is designed to be **hardlinked** under different names.
//! When executed, it discovers its own filename, reads the corresponding
//! `.shim` config file in the same directory, and launches the target
//! executable with all passed arguments.

use std::{env, fs, process};

fn main() {
    let own_path = env::current_exe().expect("bpm-shim: cannot determine own executable path");
    let dir = own_path.parent().expect("bpm-shim: no parent directory");
    let stem = own_path
        .file_stem()
        .expect("bpm-shim: no file stem")
        .to_str()
        .expect("bpm-shim: stem is not valid UTF-8");

    let shim_path = dir.join(format!("{stem}.shim"));
    let content = match fs::read_to_string(&shim_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("bpm-shim: failed to read {}: {e}", shim_path.display());
            process::exit(1);
        }
    };

    let mut target_path = String::new();
    let mut extra_args = String::new();

    for line in content.lines() {
        let line = line.trim();
        // Parse `key = value` (or `key=value`), values optionally quoted.
        // A bare strip_prefix("path") would also match `pathname = ...`.
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.trim().trim_matches('"');
        match key.trim() {
            "path" => target_path = value.to_string(),
            "args" => extra_args = value.to_string(),
            _ => {}
        }
    }

    if target_path.is_empty() {
        eprintln!("bpm-shim: no 'path' found in {}", shim_path.display());
        process::exit(1);
    }

    let mut cmd = process::Command::new(&target_path);
    if !extra_args.is_empty() {
        cmd.arg(&extra_args);
    }
    cmd.args(env::args().skip(1));

    match cmd.status() {
        Ok(status) => process::exit(status.code().unwrap_or(1)),
        Err(e) => {
            eprintln!("bpm-shim: failed to execute '{}': {e}", target_path);
            process::exit(1);
        }
    }
}
