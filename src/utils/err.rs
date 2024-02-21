use super::error_exit;

pub fn invalid_asset_error() -> ! {
    error_exit!("No available asset found in this repo. If you're sure there's a valid asset, use `--interactive`.")
}
