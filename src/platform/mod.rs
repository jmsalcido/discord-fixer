//! Finding Discord installations. One module per platform; each exposes
//! `discover() -> Vec<Install>`, returning only installs that actually exist.

use crate::discord::Install;
use std::path::{Path, PathBuf};

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::discover;

#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
pub use windows::discover;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::discover;

/// Discover installs, sorted so Stable comes first — it's what almost everyone
/// is actually here to fix.
pub fn discover_sorted() -> Vec<Install> {
    let mut installs = discover();
    installs.sort_by_key(|i| {
        crate::discord::Flavor::ALL
            .iter()
            .position(|f| *f == i.flavor)
            .unwrap_or(usize::MAX)
    });
    installs
}

/// First entry on `PATH` named `program` that we can actually execute.
#[allow(dead_code)]
pub(crate) fn which(program: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(program))
        .find(|candidate| is_executable(candidate))
}

#[allow(dead_code)]
fn is_executable(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path)
            .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        path.is_file()
    }
}
