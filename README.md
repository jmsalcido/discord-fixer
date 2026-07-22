<div align="center">

<img src="assets/icon.png" width="112" alt="">

# Discord Desktop Fixer

**Discord stuck on a grey screen? Blank window? Endless "Checking for updates"?**
Download, open, click one button.

[**Download for macOS, Windows or Linux →**](../../releases/latest)

</div>

---

## What it does

Discord is an Electron app, and Electron apps accumulate corrupt caches. When
that happens the client hangs, renders nothing, or loops forever on an update it
can't finish. The fix has always been the same: close Discord, delete its cache
directories, open it again.

This does exactly that, with no terminal and nothing to install.

**You stay logged in.** The files holding your session — `Local Storage` and
`Cookies` — are on a hard-coded deny list that neither cleaning mode can touch,
and [a test](src/clean.rs) fails the build if that ever stops being true. Your
messages live on Discord's servers and are never involved.

## Using it

| Platform | What to download | How to open it |
|---|---|---|
| **macOS** 10.15+ | `.dmg` | Drag to Applications, then open it. Official releases are signed and notarized by Apple, so there's no warning. |
| **Windows** 10/11 | `.exe` | Just run it — no installer. Windows shows a SmartScreen warning; see below. |
| **Linux** | `.AppImage` | `chmod +x` it, then double-click. |

Then: click **Fix Discord**. It closes Discord, clears the caches, and reopens
it. Usually a couple of seconds.

### Deep clean

The checkbox does more: it also removes Discord's downloaded native modules
(often 200–300 MB) and its web storage. Discord re-downloads them on the next
launch, which takes a moment longer but is what actually breaks a client out of
a stuck-update loop.

You stay logged in with this too.

### If macOS says it "cannot be opened"

Only applies to unsigned builds — your own `bundle.sh` output, or a release cut
without Apple credentials configured. Notarized releases open normally.

macOS 15 removed the old right-click → **Open** trick. The route now is:

1. Try to open the app; let the warning appear, and dismiss it.
2. **System Settings → Privacy & Security**, scroll to the bottom.
3. Click **Open Anyway** next to the app's name, then confirm.

Or, from a terminal: `xattr -d com.apple.quarantine "/Applications/Discord Desktop Fixer.app"`

### Windows SmartScreen

The Windows build isn't code-signed yet — a certificate costs a few hundred
dollars a year — so Windows shows *"Windows protected your PC"* the first time.
Click **More info → Run anyway**. If that isn't good enough for you, that's a
completely reasonable position: build it yourself with the instructions below.

## What gets deleted

Everything is inside Discord's own data directory
(`~/Library/Application Support/discord` on macOS,
`%AppData%\discord` on Windows, `~/.config/discord` on Linux).

<table>
<tr><th align="left">Always</th><th align="left">Deep clean also</th><th align="left">Never</th></tr>
<tr valign="top"><td>

`Cache`
`Code Cache`
`GPUCache`
`DawnGraphiteCache`
`DawnWebGPUCache`
`GrShaderCache`
`ShaderCache`
`blob_storage`
`component_crx_cache`
`Crashpad`
`logs`
`sentry`
`Shared Dictionary`

</td><td>

`Service Worker`
`Session Storage`
`SharedStorage`
`DIPS`
`Network Persistent State`
`Trust Tokens`
`WebStorage`
`modules`
`<version>/modules`

</td><td>

`Local Storage` ← your login
`Cookies` ← your login
`Cookies-journal`
`SingletonCookie`
`settings.json`
`quotes.json`

</td></tr>
</table>

Discord Stable, PTB, Canary and Development are each detected separately, along
with Flatpak and Snap installs on Linux. If you have more than one, you pick
which to clean.

## Command line

The same binary works headlessly, which is handy for scripting or if you just
don't trust a button.

```bash
discord-desktop-fixer --dry-run
```

That prints exactly what would be removed, and its size, without deleting
anything.

```
--cli            Run in the terminal instead of opening a window
--dry-run        Show what would be cleared, delete nothing (implies --cli)
--deep           Also clear stored modules and web storage
--no-relaunch    Don't reopen Discord afterwards
```

## Building it yourself

Needs [Rust](https://rustup.rs).

```bash
cargo run                  # the app
cargo test                 # includes the never-log-you-out tests
cargo build --release      # a single self-contained binary
```

Platform packages:

```bash
./packaging/macos/bundle.sh      # universal .app + .dmg, optionally signed
./packaging/linux/appimage.sh    # .AppImage
```

`bundle.sh` signs and notarizes when you give it `SIGN_IDENTITY` and Apple
credentials, and produces a working unsigned build when you don't. See the
comments at the top of the script.

On Linux you'll need the usual GUI headers:

```bash
sudo apt install libx11-dev libxcursor-dev libxrandr-dev libxi-dev libgl1-mesa-dev libxkbcommon-dev libwayland-dev
```

### Releasing

Push a tag. [The workflow](.github/workflows/release.yml) runs the test suite on
all three platforms, then builds, signs, notarizes and publishes everything.

```bash
git tag v0.1.0 && git push --tags
```

macOS signing needs five repository secrets: `MACOS_CERT_P12` (base64 of a
Developer ID Application `.p12`), `MACOS_CERT_PASSWORD`, `APPLE_ID`,
`APPLE_TEAM_ID`, `APPLE_APP_PASSWORD` (an app-specific password).

**Without them the release still builds and publishes**, on every platform — the
signing and notarization steps skip themselves. The only difference is that
macOS users get a Gatekeeper warning and have to click through
[Open Anyway](#if-macos-says-it-cannot-be-opened). Windows and Linux are
unaffected either way, since neither is signed regardless.

## How it's built

A single Rust binary using [egui](https://github.com/emilk/egui). No Electron,
no webview, no runtime to install — about 6 MB, and it starts instantly.

Some notes on the parts that matter:

- **Process matching is on the executable path, never the command line.** The
  shell script this replaces used `pkill -f Discord`, which also matches any
  unrelated process whose *arguments* happen to mention Discord.
- **Deletion passes four gates**: a deny-list check, a symlink refusal, a
  containment check that the target is a direct child of a directory we
  discovered ourselves (after resolving symlinks), and a root/home guard.
- **Discord gets SIGTERM before SIGKILL**, so it flushes its state, and we wait
  for `SingletonLock` to clear before deleting anything.
- **One target failing never stops the rest** — a file locked by a straggling
  process shouldn't block the other thirteen deletions.

## License

MIT.
