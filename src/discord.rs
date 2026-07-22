//! The model of what a Discord installation looks like on disk, and exactly
//! which parts of it are safe to delete.
//!
//! The single most important invariant in this program lives here: the `NEVER`
//! list. Discord keeps its auth token in `Local Storage` and `Cookies`. If we
//! ever delete those, the user gets logged out — which is a much worse day than
//! the one they were already having. Nothing in `SAFE` or `DEEP` may overlap
//! with `NEVER`, and there is a test that enforces it.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Flavor {
    Stable,
    Ptb,
    Canary,
    Development,
}

impl Flavor {
    pub const ALL: [Flavor; 4] = [
        Flavor::Stable,
        Flavor::Ptb,
        Flavor::Canary,
        Flavor::Development,
    ];

    /// What we show the user.
    pub fn display_name(self) -> &'static str {
        match self {
            Flavor::Stable => "Discord",
            Flavor::Ptb => "Discord PTB",
            Flavor::Canary => "Discord Canary",
            Flavor::Development => "Discord Development",
        }
    }

    /// The Electron userData directory name, used on every platform.
    pub fn data_dir_name(self) -> &'static str {
        match self {
            Flavor::Stable => "discord",
            Flavor::Ptb => "discordptb",
            Flavor::Canary => "discordcanary",
            Flavor::Development => "discorddevelopment",
        }
    }

    pub fn macos_bundle_id(self) -> &'static str {
        match self {
            Flavor::Stable => "com.hnc.Discord",
            Flavor::Ptb => "com.hnc.DiscordPTB",
            Flavor::Canary => "com.hnc.DiscordCanary",
            Flavor::Development => "com.hnc.DiscordDevelopment",
        }
    }

    /// Base name of the main executable and the stem its helper processes share.
    ///
    /// macOS: `Discord`, `Discord Helper`, `Discord Helper (GPU)`, …
    /// Linux: `Discord`, `DiscordCanary`, …
    /// Windows: `Discord.exe`, …
    pub fn process_stem(self) -> &'static str {
        if cfg!(target_os = "macos") {
            match self {
                Flavor::Stable => "Discord",
                Flavor::Ptb => "Discord PTB",
                Flavor::Canary => "Discord Canary",
                Flavor::Development => "Discord Development",
            }
        } else {
            match self {
                Flavor::Stable => "Discord",
                Flavor::Ptb => "DiscordPTB",
                Flavor::Canary => "DiscordCanary",
                Flavor::Development => "DiscordDevelopment",
            }
        }
    }

    /// Does this process belong to this flavor of Discord?
    ///
    /// Deliberately matches on the *executable path*, never on the full command
    /// line. `pkill -f Discord` — the thing this app replaces — would happily
    /// kill an editor that merely had a Discord path open in a buffer.
    pub fn matches_process(self, exe: &Path) -> bool {
        let Some(name) = exe.file_name().and_then(|n| n.to_str()) else {
            return false;
        };
        let stem = self.process_stem();
        let name = name.strip_suffix(".exe").unwrap_or(name);

        // The main process, or one of its `<stem> Helper (Renderer)` children.
        if name == stem
            || name
                .strip_prefix(stem)
                .is_some_and(|r| r.starts_with(" Helper"))
        {
            return true;
        }

        // Crashpad shares one binary name across every Electron app, so it can
        // only be attributed by where it lives.
        if name == "chrome_crashpad_handler" {
            let path = exe.to_string_lossy();
            return path.contains(&format!("{}.app/", self.display_name()))
                || path.contains(&format!("/{}/", self.data_dir_name()));
        }

        false
    }
}

// Variants below are each constructed on exactly one platform, so every build
// legitimately leaves some of them unused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum Packaging {
    Native,
    Flatpak,
    Snap,
}

/// How to start this install back up once we're done.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum LaunchSpec {
    /// `open -b com.hnc.Discord`
    MacOsBundleId(&'static str),
    /// Squirrel's stub launcher. Starting `app-x.y.z\Discord.exe` directly
    /// works but quietly breaks the auto-updater, so we always go via this.
    WindowsSquirrel {
        update_exe: PathBuf,
        exe_name: String,
    },
    Command {
        program: String,
        args: Vec<String>,
    },
}

#[derive(Debug, Clone)]
pub struct Install {
    pub flavor: Flavor,
    pub packaging: Packaging,
    /// The Electron userData dir. Named cache targets are resolved inside it.
    pub data_dir: PathBuf,
    /// Whole directories outside `data_dir` that are pure cache and get removed
    /// entirely — on macOS, `~/Library/Caches/com.hnc.Discord*`.
    pub extra_cache_dirs: Vec<PathBuf>,
    /// Same, but only removed by a deep clean (update staging areas, mostly).
    pub extra_deep_cache_dirs: Vec<PathBuf>,
    pub launch: Option<LaunchSpec>,
}

impl Install {
    pub fn label(&self) -> String {
        match self.packaging {
            Packaging::Native => self.flavor.display_name().to_string(),
            Packaging::Flatpak => format!("{} (Flatpak)", self.flavor.display_name()),
            Packaging::Snap => format!("{} (Snap)", self.flavor.display_name()),
        }
    }

    /// Directories under which this install is allowed to delete things.
    ///
    /// The clean engine requires every target to be a *direct child* of one of
    /// these, after symlink resolution. Anything else is refused.
    pub fn allowed_parents(&self) -> Vec<PathBuf> {
        let mut parents = vec![self.data_dir.clone()];
        let extras = self
            .extra_cache_dirs
            .iter()
            .chain(&self.extra_deep_cache_dirs);
        for dir in extras {
            if let Some(parent) = dir.parent() {
                parents.push(parent.to_path_buf());
            }
        }
        parents.sort();
        parents.dedup();
        parents
    }

    /// Every path this install would delete at the given scope, present or not.
    pub fn targets(&self, scope: Scope) -> Vec<PathBuf> {
        let mut targets: Vec<PathBuf> = scope
            .target_names()
            .iter()
            .map(|name| self.data_dir.join(name))
            .collect();
        targets.extend(self.extra_cache_dirs.iter().cloned());
        if scope == Scope::Deep {
            targets.extend(self.extra_deep_cache_dirs.iter().cloned());
            targets.extend(self.versioned_module_dirs());
        }
        targets
    }

    /// Discord stores its downloaded native modules under a per-host-version
    /// directory — `<data_dir>/0.0.402/modules` — which is where the real bulk
    /// lives (hundreds of megabytes) and what actually has to go to break a
    /// client out of an update loop. The top-level `modules` directory is just
    /// a small index alongside it.
    ///
    /// The version is different for every user and changes on every update, so
    /// unlike everything else these are discovered rather than hard-coded.
    fn versioned_module_dirs(&self) -> Vec<PathBuf> {
        let Ok(entries) = std::fs::read_dir(&self.data_dir) else {
            return Vec::new();
        };
        entries
            .filter_map(Result::ok)
            .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
            .filter(|e| {
                e.file_name().to_str().is_some_and(looks_like_version)
                    // Only if it really is a module store. A numeric directory
                    // without one is something we don't understand, so we leave
                    // it alone.
                    && e.path().join("modules").is_dir()
            })
            .map(|e| e.path())
            .collect()
    }
}

/// `0.0.402` yes; `0.0.402-beta`, `modules`, `1` no.
fn looks_like_version(name: &str) -> bool {
    let parts: Vec<&str> = name.split('.').collect();
    parts.len() >= 2
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// Caches only. Cannot log the user out, cannot lose settings.
    Safe,
    /// Everything in `Safe`, plus regenerable browser state and `modules`.
    /// Still cannot log the user out.
    Deep,
}

impl Scope {
    pub fn target_names(self) -> Vec<&'static str> {
        let mut names = SAFE.to_vec();
        if self == Scope::Deep {
            names.extend_from_slice(DEEP_EXTRA);
        }
        names
    }
}

/// Pure caches. Discord rebuilds all of these on its own.
///
/// Intentionally a superset across Discord versions — names come and go
/// between releases (`DawnCache` was split into `DawnGraphiteCache` and
/// `DawnWebGPUCache`, for instance), and missing entries are simply skipped.
pub const SAFE: &[&str] = &[
    "Cache",
    "Code Cache",
    "GPUCache",
    "DawnCache",
    "DawnGraphiteCache",
    "DawnWebGPUCache",
    "GrShaderCache",
    "ShaderCache",
    "blob_storage",
    "component_crx_cache",
    "Crashpad",
    "logs",
    "sentry",
    "Shared Dictionary",
];

/// Added by a deep clean. Regenerable, but heavier: dropping these logs you out
/// of *nothing*, though it does reset per-site web storage and forces Discord
/// to re-download its native modules — which is precisely what unsticks a
/// client trapped in an update loop.
pub const DEEP_EXTRA: &[&str] = &[
    "Service Worker",
    "Session Storage",
    "SharedStorage",
    "SharedStorage-wal",
    "DIPS",
    "DIPS-wal",
    "Network Persistent State",
    "Trust Tokens",
    "Trust Tokens-journal",
    "WebStorage",
    "modules",
];

/// Never, under any scope, for any reason.
///
/// `Local Storage` and `Cookies` hold the auth token; the rest is user config
/// that would be genuinely annoying to lose.
pub const NEVER: &[&str] = &[
    "Local Storage",
    "Cookies",
    "Cookies-journal",
    "SingletonCookie",
    "settings.json",
    "quotes.json",
];

/// Case-insensitive membership test against [`NEVER`].
pub fn is_never_deletable(name: &str) -> bool {
    NEVER.iter().any(|n| n.eq_ignore_ascii_case(name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deletable_lists_never_overlap_the_deny_list() {
        for name in Scope::Deep.target_names() {
            assert!(
                !is_never_deletable(name),
                "{name} is scheduled for deletion but is on the NEVER list"
            );
        }
    }

    #[test]
    fn safe_is_a_subset_of_deep() {
        let deep = Scope::Deep.target_names();
        for name in Scope::Safe.target_names() {
            assert!(deep.contains(&name), "{name} missing from deep clean");
        }
    }

    #[test]
    fn target_names_are_unique() {
        let mut names = Scope::Deep.target_names();
        let before = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(before, names.len(), "duplicate entries in target lists");
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn matches_real_macos_process_names() {
        let f = Flavor::Stable;
        assert!(f.matches_process(Path::new(
            "/Applications/Discord.app/Contents/MacOS/Discord"
        )));
        assert!(f.matches_process(Path::new("/Applications/Discord.app/Contents/Frameworks/Discord Helper (GPU).app/Contents/MacOS/Discord Helper (GPU)")));
        assert!(f.matches_process(Path::new(
            "/Applications/Discord.app/Contents/Frameworks/chrome_crashpad_handler"
        )));
    }

    #[test]
    fn does_not_match_unrelated_processes() {
        let f = Flavor::Stable;
        // The `pkill -f Discord` failure mode: a path that merely mentions Discord.
        assert!(!f.matches_process(Path::new("/usr/local/bin/node")));
        assert!(!f.matches_process(Path::new("/Users/me/dev/discord-fixer/target/debug/nvim")));
        assert!(!f.matches_process(Path::new(
            "/Applications/Discordo.app/Contents/MacOS/Discordo"
        )));
        // Canary is a different install and must not be swept up by Stable.
        assert!(!f.matches_process(Path::new(
            "/Applications/Discord Canary.app/Contents/MacOS/Discord Canary"
        )));
    }

    #[test]
    fn recognises_host_version_directory_names() {
        assert!(looks_like_version("0.0.402"));
        assert!(looks_like_version("1.0"));
        assert!(!looks_like_version("modules"));
        assert!(!looks_like_version("0.0.402-canary"));
        assert!(!looks_like_version("0..1"));
        assert!(!looks_like_version("402"));
        assert!(!looks_like_version(""));
    }

    #[test]
    fn versioned_modules_are_deep_only_and_need_a_modules_child() {
        let tmp = tempfile::tempdir().unwrap();
        let data_dir = tmp.path().join("discord");
        std::fs::create_dir_all(data_dir.join("0.0.402/modules")).unwrap();
        // A numeric directory with no `modules` inside is not ours to delete.
        std::fs::create_dir_all(data_dir.join("9.9.9/something-else")).unwrap();

        let install = Install {
            flavor: Flavor::Stable,
            packaging: Packaging::Native,
            data_dir: data_dir.clone(),
            extra_cache_dirs: Vec::new(),
            extra_deep_cache_dirs: Vec::new(),
            launch: None,
        };

        assert!(
            !install
                .targets(Scope::Safe)
                .contains(&data_dir.join("0.0.402"))
        );
        let deep = install.targets(Scope::Deep);
        assert!(deep.contains(&data_dir.join("0.0.402")));
        assert!(!deep.contains(&data_dir.join("9.9.9")));
    }

    #[test]
    fn flavors_have_distinct_identities() {
        for a in Flavor::ALL {
            for b in Flavor::ALL {
                if a != b {
                    assert_ne!(a.data_dir_name(), b.data_dir_name());
                    assert_ne!(a.macos_bundle_id(), b.macos_bundle_id());
                }
            }
        }
    }
}
