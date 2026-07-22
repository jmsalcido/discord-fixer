use crate::discord::{Flavor, Install, LaunchSpec, Packaging};
use crate::platform::which;
use std::path::PathBuf;

fn flatpak_app_id(flavor: Flavor) -> &'static str {
    match flavor {
        Flavor::Stable => "com.discordapp.Discord",
        Flavor::Ptb => "com.discordapp.DiscordPTB",
        Flavor::Canary => "com.discordapp.DiscordCanary",
        Flavor::Development => "com.discordapp.DiscordDevelopment",
    }
}

pub fn discover() -> Vec<Install> {
    let Some(home) = dirs::home_dir() else {
        return Vec::new();
    };
    let config = dirs::config_dir().unwrap_or_else(|| home.join(".config"));
    let cache = dirs::cache_dir().unwrap_or_else(|| home.join(".cache"));

    let mut installs = Vec::new();

    for &flavor in &Flavor::ALL {
        let name = flavor.data_dir_name();

        // Native tarball / .deb install.
        let native = config.join(name);
        if native.is_dir() {
            let extra = cache.join(name);
            installs.push(Install {
                flavor,
                packaging: Packaging::Native,
                data_dir: native,
                extra_cache_dirs: if extra.is_dir() { vec![extra] } else { vec![] },
                extra_deep_cache_dirs: Vec::new(),
                launch: which(flavor.process_stem())
                    .or_else(|| which(&flavor.process_stem().to_lowercase()))
                    .map(|p| LaunchSpec::Command {
                        program: p.to_string_lossy().into_owned(),
                        args: Vec::new(),
                    }),
            });
        }

        // Flatpak keeps a private HOME per app.
        let app_id = flatpak_app_id(flavor);
        let flatpak_root = home.join(".var/app").join(app_id);
        let flatpak_data = flatpak_root.join("config").join(name);
        if flatpak_data.is_dir() {
            let extra = flatpak_root.join("cache").join(name);
            installs.push(Install {
                flavor,
                packaging: Packaging::Flatpak,
                data_dir: flatpak_data,
                extra_cache_dirs: if extra.is_dir() { vec![extra] } else { vec![] },
                extra_deep_cache_dirs: Vec::new(),
                launch: which("flatpak").map(|p| LaunchSpec::Command {
                    program: p.to_string_lossy().into_owned(),
                    args: vec!["run".into(), app_id.into()],
                }),
            });
        }

        // Snap. Only stable is published, but the layout is uniform.
        let snap_data: PathBuf = home
            .join("snap")
            .join(name)
            .join("current/.config")
            .join(name);
        if snap_data.is_dir() {
            installs.push(Install {
                flavor,
                packaging: Packaging::Snap,
                data_dir: snap_data,
                extra_cache_dirs: Vec::new(),
                extra_deep_cache_dirs: Vec::new(),
                launch: which("snap").map(|p| LaunchSpec::Command {
                    program: p.to_string_lossy().into_owned(),
                    args: vec!["run".into(), name.into()],
                }),
            });
        }
    }

    installs
}
