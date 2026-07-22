use crate::discord::{Flavor, Install, LaunchSpec, Packaging};
use std::path::PathBuf;

pub fn discover() -> Vec<Install> {
    let Some(home) = dirs::home_dir() else {
        return Vec::new();
    };
    let app_support = home.join("Library/Application Support");
    let caches = home.join("Library/Caches");
    let saved_state = home.join("Library/Saved Application State");

    Flavor::ALL
        .iter()
        .filter_map(|&flavor| {
            let data_dir = app_support.join(flavor.data_dir_name());
            if !data_dir.is_dir() {
                return None;
            }
            let bundle_id = flavor.macos_bundle_id();

            let extra_cache_dirs = existing(vec![
                caches.join(bundle_id),
                // Squirrel.Mac's update staging cache. Stale contents here are a
                // classic cause of the "Checking for updates…" loop.
                caches.join(format!("{bundle_id}.ShipIt")),
                // Window/restore state. Regenerated on next launch; clearing it
                // fixes the "opens as a blank grey rectangle" case.
                saved_state.join(format!("{bundle_id}.savedState")),
            ]);

            Some(Install {
                flavor,
                packaging: Packaging::Native,
                data_dir,
                extra_cache_dirs,
                extra_deep_cache_dirs: Vec::new(),
                launch: Some(LaunchSpec::MacOsBundleId(bundle_id)),
            })
        })
        .collect()
}

fn existing(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    paths.into_iter().filter(|p| p.exists()).collect()
}
