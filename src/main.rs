// No console window should flash up when the app is double-clicked on Windows.
// `--cli` reattaches to the parent console explicitly (see `attach_console`).
#![cfg_attr(windows, windows_subsystem = "windows")]

mod app;
mod clean;
mod discord;
mod job;
mod kill;
mod launch;
mod platform;

use discord::Scope;
use job::Options;

const HELP: &str = "\
Discord Desktop Fixer — clears Discord's caches when it gets stuck.

USAGE:
    discord-desktop-fixer [OPTIONS]

Run with no options to open the app.

OPTIONS:
    --cli            Run in the terminal instead of opening a window
    --dry-run        Show what would be cleared without deleting anything
                     (implies --cli)
    --deep           Also clear stored modules and web storage, which fixes
                     clients stuck on \"Checking for updates\". You stay
                     logged in either way.
    --no-relaunch    Don't reopen Discord afterwards
    -h, --help       Print this help
    -V, --version    Print version
";

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let has = |flag: &str| args.iter().any(|a| a == flag);

    if has("-h") || has("--help") {
        attach_console();
        println!("{HELP}");
        return std::process::ExitCode::SUCCESS;
    }
    if has("-V") || has("--version") {
        attach_console();
        println!("discord-desktop-fixer {}", env!("CARGO_PKG_VERSION"));
        return std::process::ExitCode::SUCCESS;
    }

    if let Some(unknown) = args.iter().find(|a| {
        !matches!(
            a.as_str(),
            "--cli" | "--dry-run" | "--deep" | "--no-relaunch"
        )
    }) {
        attach_console();
        eprintln!("unrecognised option: {unknown}\n\n{HELP}");
        return std::process::ExitCode::FAILURE;
    }

    let dry_run = has("--dry-run");
    let options = Options {
        scope: if has("--deep") {
            Scope::Deep
        } else {
            Scope::Safe
        },
        relaunch: !has("--no-relaunch"),
        dry_run,
    };

    if has("--cli") || dry_run {
        attach_console();
        run_cli(options)
    } else {
        match app::run(options) {
            Ok(()) => std::process::ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("{e}");
                std::process::ExitCode::FAILURE
            }
        }
    }
}

fn run_cli(options: Options) -> std::process::ExitCode {
    let installs = platform::discover_sorted();
    if installs.is_empty() {
        eprintln!("No Discord installation found.");
        return std::process::ExitCode::FAILURE;
    }

    for install in &installs {
        println!(
            "Found {} at {}",
            install.label(),
            install.data_dir.display()
        );
    }
    if options.dry_run {
        println!("\nDry run — nothing will be deleted.");
    }
    println!();

    let summary = job::run(&installs, options, |step| println!("  {}", step.0));

    // A dry run is only useful if it says what it would have done.
    if options.dry_run {
        println!("\nWould remove:");
        for item in &summary.report.items {
            if let clean::Outcome::Removed { bytes } = item.outcome {
                println!(
                    "  {:>10}  {}",
                    clean::format_bytes(bytes),
                    item.path.display()
                );
            }
        }
    }

    println!("\n{}", summary.headline(options.dry_run));
    if !summary.warnings.is_empty() {
        println!("\nWarnings:");
        for warning in &summary.warnings {
            println!("  - {warning}");
        }
    }
    println!("You are still logged in to Discord.");

    std::process::ExitCode::SUCCESS
}

/// On Windows the binary is built as a GUI subsystem app so double-clicking it
/// doesn't flash a console. That also means `--cli` has nowhere to print, so we
/// borrow the console of whatever shell launched us.
#[cfg(windows)]
fn attach_console() {
    const ATTACH_PARENT_PROCESS: u32 = u32::MAX;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn AttachConsole(dwProcessId: u32) -> i32;
    }
    unsafe {
        AttachConsole(ATTACH_PARENT_PROCESS);
    }
}

#[cfg(not(windows))]
fn attach_console() {}
