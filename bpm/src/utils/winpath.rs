//! This module provides a wrapper for Windows PATH operations.
//! It is used to add and remove paths from the Windows PATH environment
//! variable.

use std::io;

use log::{debug, info, warn};
use windows::{
    core::{HSTRING, PCWSTR},
    Win32::{
        Foundation::LPARAM,
        UI::WindowsAndMessaging::{
            SendMessageTimeoutW, HWND_BROADCAST, SMTO_ABORTIFHUNG, WM_SETTINGCHANGE,
        },
    },
};
use winreg::{
    enums::{HKEY_CURRENT_USER, KEY_READ, KEY_WRITE},
    RegKey,
};

/// Add a path to the Windows PATH environment variable.
///
/// # Errors
///
/// Returns an error if the path could not be added to the PATH environment
/// variable.
pub fn add_to_env_path(new_path: &str) -> io::Result<()> {
    modify_path(new_path, true)
}

/// Remove a path from the Windows PATH environment variable.
///
/// # Errors
///
/// Returns an error if the path could not be removed from the PATH environment
/// variable.
pub fn remove_from_env_path(target_path: &str) -> io::Result<()> {
    modify_path(target_path, false)
}

fn modify_path(target_path: &str, add: bool) -> io::Result<()> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let env = hkcu.open_subkey_with_flags("Environment", KEY_READ | KEY_WRITE)?;

    let current_path: String = env.get_value("PATH").unwrap_or_else(|_| String::new());

    let paths: Vec<&str> = current_path.split(';').collect();
    let path_exists = paths.iter().any(|p| p == &target_path);

    let new_path_value = if add {
        if path_exists {
            debug!("Path already exists in the PATH variable.");
            return Ok(());
        } else if current_path.is_empty() {
            target_path.to_string()
        } else {
            format!("{current_path};{target_path}")
        }
    } else if path_exists {
        paths
            .into_iter()
            .filter(|p| p != &target_path)
            .collect::<Vec<&str>>()
            .join(";")
    } else {
        warn!("Path not found in the PATH variable.");
        return Ok(());
    };

    env.set_value("PATH", &new_path_value)?;
    notify_system();
    info!(
        "PATH {} successfully: `{}`",
        if add { "added" } else { "removed" },
        target_path
    );

    Ok(())
}

/// Convert UTF-8 str to PCWSTR
macro_rules! w {
    ($x: expr) => {
        PCWSTR::from_raw(HSTRING::from($x).as_ptr())
    };
}

fn notify_system() {
    let msg = w!("Environment");
    unsafe {
        SendMessageTimeoutW(
            HWND_BROADCAST,
            WM_SETTINGCHANGE,
            None,
            LPARAM(msg.as_ptr() as isize),
            SMTO_ABORTIFHUNG,
            500,
            None,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add_and_remove_path() {
        let test_path = r"C:\test\path";

        // Add test path
        add_to_env_path(test_path).expect("Failed to add path");
        // Check if the path was added successfully
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let env = hkcu.open_subkey("Environment").unwrap();
        let current_path: String = env.get_value("PATH").unwrap();
        assert!(current_path.contains(test_path));

        // Remove test path
        remove_from_env_path(test_path).expect("Failed to remove path");
        // Check if the path was removed successfully
        let current_path: String = env.get_value("PATH").unwrap();
        assert!(!current_path.contains(test_path));
    }
}
