use crate::discord::{Flavor, Install, LaunchSpec, Packaging};

/// Squirrel installs Discord per-user under `%LocalAppData%\<Name>`, with the
/// Electron userData living separately under `%AppData%\<name>`.
fn install_dir_name(flavor: Flavor) -> &'static str {
    match flavor {
        Flavor::Stable => "Discord",
        Flavor::Ptb => "DiscordPTB",
        Flavor::Canary => "DiscordCanary",
        Flavor::Development => "DiscordDevelopment",
    }
}

pub fn discover() -> Vec<Install> {
    // On Windows `dirs::config_dir()` is Roaming AppData and `data_local_dir()`
    // is Local AppData.
    let (Some(roaming), Some(local)) = (dirs::config_dir(), dirs::data_local_dir()) else {
        return Vec::new();
    };

    Flavor::ALL
        .iter()
        .filter_map(|&flavor| {
            let data_dir = roaming.join(flavor.data_dir_name());
            if !data_dir.is_dir() {
                return None;
            }

            let install_dir = local.join(install_dir_name(flavor));
            let update_exe = install_dir.join("Update.exe");
            let launch = update_exe.is_file().then(|| LaunchSpec::WindowsSquirrel {
                update_exe,
                exe_name: format!("{}.exe", install_dir_name(flavor)),
            });

            // Downloaded update packages. Safe to drop — Squirrel re-fetches
            // them — but they're big and only worth touching on a deep clean.
            let packages = install_dir.join("packages");
            let extra_deep_cache_dirs = if packages.is_dir() {
                vec![packages]
            } else {
                vec![]
            };

            Some(Install {
                flavor,
                packaging: Packaging::Native,
                data_dir,
                extra_cache_dirs: Vec::new(),
                extra_deep_cache_dirs,
                launch,
            })
        })
        .collect()
}
