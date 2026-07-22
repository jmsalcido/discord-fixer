//! The deletion engine.
//!
//! Everything here is written on the assumption that a bug in this file
//! destroys someone's data. Each target passes four independent gates before a
//! single byte is removed, and a failure on one target never aborts the rest of
//! the run — a file locked by a straggling Discord process on Windows
//! shouldn't stop the other thirteen deletions from happening.

use crate::discord::{Install, Scope, is_never_deletable};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Deleted (or, in a dry run, would have been).
    Removed { bytes: u64 },
    /// Not on disk. Expected and unremarkable — the target lists are a superset
    /// across Discord versions.
    NotPresent,
    /// A guard rail said no. Always a bug or something genuinely strange on
    /// disk; surfaced loudly rather than swallowed.
    Refused(String),
    /// We tried and the filesystem said no.
    Failed(String),
}

#[derive(Debug, Clone)]
pub struct Item {
    pub path: PathBuf,
    pub outcome: Outcome,
}

#[derive(Debug, Clone, Default)]
pub struct Report {
    pub items: Vec<Item>,
}

impl Report {
    pub fn bytes_freed(&self) -> u64 {
        self.items
            .iter()
            .map(|i| match i.outcome {
                Outcome::Removed { bytes } => bytes,
                _ => 0,
            })
            .sum()
    }

    pub fn removed_count(&self) -> usize {
        self.items
            .iter()
            .filter(|i| matches!(i.outcome, Outcome::Removed { .. }))
            .count()
    }

    /// Anything the user might want to know went wrong.
    pub fn problems(&self) -> Vec<&Item> {
        self.items
            .iter()
            .filter(|i| matches!(i.outcome, Outcome::Refused(_) | Outcome::Failed(_)))
            .collect()
    }

    pub fn merge(&mut self, other: Report) {
        self.items.extend(other.items);
    }
}

/// Delete every cache target for `install` at `scope`.
///
/// With `dry_run` set, sizes are still measured and every gate still runs — the
/// only difference is that nothing is removed. That makes `--dry-run` a real
/// rehearsal rather than an approximation.
pub fn clean(install: &Install, scope: Scope, dry_run: bool) -> Report {
    let allowed = canonical_parents(install);
    let mut report = Report::default();

    for target in install.targets(scope) {
        let outcome = clean_one(&target, &allowed, dry_run);
        report.items.push(Item {
            path: target,
            outcome,
        });
    }

    report
}

fn clean_one(target: &Path, allowed: &[PathBuf], dry_run: bool) -> Outcome {
    // Deliberately `symlink_metadata`: we want to know about the link itself,
    // not whatever it points at.
    let meta = match std::fs::symlink_metadata(target) {
        Ok(meta) => meta,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Outcome::NotPresent,
        Err(e) => return Outcome::Failed(e.to_string()),
    };

    if let Err(reason) = guard(target, &meta, allowed) {
        return Outcome::Refused(reason);
    }

    let bytes = measure(target);
    if dry_run {
        return Outcome::Removed { bytes };
    }

    let result = if meta.is_dir() {
        std::fs::remove_dir_all(target)
    } else {
        std::fs::remove_file(target)
    };

    match result {
        Ok(()) => Outcome::Removed { bytes },
        Err(e) => Outcome::Failed(e.to_string()),
    }
}

/// The four gates. Any `Err` means we do not touch this path.
fn guard(target: &Path, meta: &std::fs::Metadata, allowed: &[PathBuf]) -> Result<(), String> {
    // 1. It must be a named child of something, not a bare root.
    let Some(name) = target.file_name().and_then(|n| n.to_str()) else {
        return Err("path has no file name".into());
    };

    // 2. The deny list. This is the guarantee that we never log the user out.
    if is_never_deletable(name) {
        return Err(format!("{name} is on the never-delete list"));
    }

    // 3. Symlinks are refused outright rather than followed. A link is never
    //    something Discord's cache legitimately consists of, and following one
    //    is how a delete escapes its directory.
    if meta.is_symlink() {
        return Err("refusing to follow a symlink".into());
    }

    // 4. Containment. Canonicalize the parent — which resolves any symlinked
    //    component along the way — and require an exact match against a parent
    //    we discovered ourselves. Being a *direct child* is required; nothing
    //    nested, nothing above.
    let Some(parent) = target.parent() else {
        return Err("path has no parent".into());
    };
    let canonical_parent = parent
        .canonicalize()
        .map_err(|e| format!("cannot resolve parent directory: {e}"))?;

    if !allowed.contains(&canonical_parent) {
        return Err(format!(
            "{} is outside every known Discord directory",
            canonical_parent.display()
        ));
    }

    // Belt and braces: never let a target resolve to a home directory or a
    // filesystem root, whatever the above concluded.
    let resolved = canonical_parent.join(name);
    if resolved.parent().is_none() || Some(resolved.as_path()) == dirs::home_dir().as_deref() {
        return Err("refusing to delete a root or home directory".into());
    }

    Ok(())
}

/// Allowed parents, canonicalized once and with the non-existent ones dropped.
fn canonical_parents(install: &Install) -> Vec<PathBuf> {
    install
        .allowed_parents()
        .iter()
        .filter_map(|p| p.canonicalize().ok())
        .collect()
}

/// Total bytes on disk. Never follows links, and ignores entries it can't stat
/// rather than giving up — this is a progress number, not an audit.
fn measure(path: &Path) -> u64 {
    WalkDir::new(path)
        .follow_links(false)
        .into_iter()
        .filter_map(Result::ok)
        .filter_map(|e| e.metadata().ok())
        .filter(|m| m.is_file())
        .map(|m| m.len())
        .sum()
}

pub fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.2} {}", UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discord::{Flavor, Packaging};
    use std::fs;

    /// A fixture mirroring the layout observed on a real macOS install,
    /// including the auth-bearing directories we must never touch.
    fn fixture(root: &Path) -> Install {
        let data_dir = root.join("discord");
        for dir in [
            "Cache",
            "Code Cache",
            "GPUCache",
            "DawnGraphiteCache",
            "DawnWebGPUCache",
            "blob_storage",
            "Crashpad",
            "logs",
            "sentry",
            "Shared Dictionary",
            "Service Worker",
            "Session Storage",
            "modules",
            "Local Storage",
            "Cookies",
        ] {
            fs::create_dir_all(data_dir.join(dir)).unwrap();
            fs::write(data_dir.join(dir).join("blob.bin"), vec![0u8; 1024]).unwrap();
        }
        fs::write(data_dir.join("settings.json"), b"{}").unwrap();
        fs::write(data_dir.join("quotes.json"), b"[]").unwrap();

        let caches = root.join("Caches");
        fs::create_dir_all(caches.join("com.hnc.Discord")).unwrap();
        fs::write(caches.join("com.hnc.Discord/x.bin"), vec![0u8; 2048]).unwrap();

        Install {
            flavor: Flavor::Stable,
            packaging: Packaging::Native,
            data_dir,
            extra_cache_dirs: vec![caches.join("com.hnc.Discord")],
            extra_deep_cache_dirs: Vec::new(),
            launch: None,
        }
    }

    /// The headline guarantee: neither scope may ever remove the auth token or
    /// the user's settings. If this test fails, the app logs people out.
    #[test]
    fn never_deletes_auth_or_settings() {
        for scope in [Scope::Safe, Scope::Deep] {
            let tmp = tempfile::tempdir().unwrap();
            let install = fixture(tmp.path());

            clean(&install, scope, false);

            for survivor in ["Local Storage", "Cookies", "settings.json", "quotes.json"] {
                assert!(
                    install.data_dir.join(survivor).exists(),
                    "{survivor} was deleted by a {scope:?} clean"
                );
            }
        }
    }

    #[test]
    fn safe_clean_removes_caches_but_leaves_deep_targets() {
        let tmp = tempfile::tempdir().unwrap();
        let install = fixture(tmp.path());

        let report = clean(&install, Scope::Safe, false);

        assert!(!install.data_dir.join("Cache").exists());
        assert!(!install.data_dir.join("DawnGraphiteCache").exists());
        assert!(!tmp.path().join("Caches/com.hnc.Discord").exists());
        // Deep-only targets survive a safe clean.
        assert!(install.data_dir.join("modules").exists());
        assert!(install.data_dir.join("Service Worker").exists());
        assert!(report.bytes_freed() > 0);
        assert!(report.problems().is_empty(), "{:?}", report.problems());
    }

    #[test]
    fn deep_clean_removes_modules_and_service_worker() {
        let tmp = tempfile::tempdir().unwrap();
        let install = fixture(tmp.path());

        clean(&install, Scope::Deep, false);

        assert!(!install.data_dir.join("modules").exists());
        assert!(!install.data_dir.join("Service Worker").exists());
    }

    #[test]
    fn dry_run_changes_nothing_but_still_measures() {
        let tmp = tempfile::tempdir().unwrap();
        let install = fixture(tmp.path());

        let report = clean(&install, Scope::Deep, true);

        assert!(install.data_dir.join("Cache").exists());
        assert!(install.data_dir.join("modules").exists());
        assert!(
            report.bytes_freed() > 0,
            "dry run should still report sizes"
        );
    }

    #[test]
    fn missing_targets_are_reported_not_failed() {
        let tmp = tempfile::tempdir().unwrap();
        let install = fixture(tmp.path());
        // ShaderCache is in SAFE but absent from the fixture, as it is on a
        // real modern install.
        let report = clean(&install, Scope::Safe, false);
        let shader = report
            .items
            .iter()
            .find(|i| i.path.ends_with("ShaderCache"))
            .expect("ShaderCache should appear in the report");
        assert_eq!(shader.outcome, Outcome::NotPresent);
    }

    #[test]
    fn refuses_a_symlink_escaping_the_data_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let install = fixture(tmp.path());

        let precious = tmp.path().join("precious");
        fs::create_dir_all(&precious).unwrap();
        fs::write(precious.join("irreplaceable.txt"), b"hello").unwrap();

        // Replace a legitimate cache target with a link pointing out of the tree.
        let target = install.data_dir.join("GPUCache");
        fs::remove_dir_all(&target).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&precious, &target).unwrap();
        #[cfg(windows)]
        std::os::windows::fs::symlink_dir(&precious, &target).unwrap();

        let report = clean(&install, Scope::Safe, false);

        assert!(
            precious.join("irreplaceable.txt").exists(),
            "followed a symlink out of the tree"
        );
        let item = report
            .items
            .iter()
            .find(|i| i.path.ends_with("GPUCache"))
            .unwrap();
        assert!(
            matches!(item.outcome, Outcome::Refused(_)),
            "{:?}",
            item.outcome
        );
    }

    #[test]
    fn refuses_targets_outside_the_allowed_parents() {
        let tmp = tempfile::tempdir().unwrap();
        let install = fixture(tmp.path());
        let allowed = canonical_parents(&install);

        let outsider = tmp.path().join("elsewhere");
        fs::create_dir_all(&outsider).unwrap();
        let victim = outsider.join("Cache");
        fs::create_dir_all(&victim).unwrap();

        assert!(matches!(
            clean_one(&victim, &allowed, false),
            Outcome::Refused(_)
        ));
        assert!(victim.exists());
    }

    #[test]
    fn refuses_the_data_dir_itself_and_traversal_out_of_it() {
        let tmp = tempfile::tempdir().unwrap();
        let install = fixture(tmp.path());
        let allowed = canonical_parents(&install);

        // The root of the install is not a child of any allowed parent.
        assert!(matches!(
            clean_one(&install.data_dir, &allowed, false),
            Outcome::Refused(_)
        ));
        assert!(install.data_dir.exists());

        // `..` climbs out, so the canonical parent stops matching.
        let traversal = install.data_dir.join("Cache/../../Caches");
        assert!(matches!(
            clean_one(&traversal, &allowed, false),
            Outcome::Refused(_)
        ));
        assert!(tmp.path().join("Caches").exists());
    }

    #[test]
    fn refuses_deny_listed_names_even_when_asked_directly() {
        let tmp = tempfile::tempdir().unwrap();
        let install = fixture(tmp.path());
        let allowed = canonical_parents(&install);

        for name in crate::discord::NEVER {
            let path = install.data_dir.join(name);
            if !path.exists() {
                continue;
            }
            assert!(
                matches!(clean_one(&path, &allowed, false), Outcome::Refused(_)),
                "{name} was not refused"
            );
            assert!(path.exists());
        }
    }

    #[test]
    fn formats_bytes_readably() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(1024), "1.00 KB");
        assert_eq!(format_bytes(1024 * 1024 * 3 / 2), "1.50 MB");
        assert_eq!(format_bytes(1024 * 1024 * 1024), "1.00 GB");
    }
}
