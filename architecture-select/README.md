# architecture-select

Select the best match item from a list, based on current platform and architecture.

It's used for github assets selection.

## Example

```rust
use architecture_select::select;

let assets = [
    "typstyle-alpine-x64",
    "typstyle-alpine-x64.debug",
    "typstyle-darwin-arm64",
    "typstyle-darwin-arm64.dwarf",
    "typstyle-darwin-x64",
    "typstyle-darwin-x64.dwarf",
    "typstyle-linux-arm64",
    "typstyle-linux-arm64.debug",
    "typstyle-linux-armhf",
    "typstyle-linux-armhf.debug",
    "typstyle-linux-x64",
    "typstyle-linux-x64.debug",
    "typstyle-win32-arm64.exe",
    "typstyle-win32-arm64.pdb",
    "typstyle-win32-x64.exe",
    "typstyle-win32-x64.pdb",
]
.map(ToOwned::to_owned)
.to_vec();

let selected_assets = select(assets);
assert!(!selected_assets.is_empty());
if cfg!(windows) {
    assert_eq!(selected_assets[0], "typstyle-win32-x64.exe");
}
```
