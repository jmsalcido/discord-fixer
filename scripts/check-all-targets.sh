#!/usr/bin/env bash
#
# Compile-check the project for every platform CI builds on, from one machine.
#
#   ./scripts/check-all-targets.sh
#
# Large parts of this codebase are behind `#[cfg(target_os = ...)]`, so a clean
# build on your host proves almost nothing about the other two platforms —
# dead-code warnings, missing `cfg` gates and platform-only type errors only
# show up under the actual target. `cargo check --target` needs just the target
# `rust-std` (no cross-linker), so this works without a full cross toolchain.
#
# Requires rustup with the targets installed:
#   rustup target add x86_64-unknown-linux-gnu x86_64-pc-windows-msvc x86_64-apple-darwin
#
# Homebrew's `rust` formula owns /opt/homebrew/bin/{cargo,rustc}. Since cargo
# finds `rustc` through PATH, that shadows rustup even when you invoke rustup's
# own cargo — so the cross targets you `rustup target add` are silently ignored
# and every build fails with "can't find crate for std". The reliable fix is to
# pin BOTH the toolchain bin (front of PATH) and RUSTC explicitly.

set -uo pipefail
cd "$(dirname "$0")/.."

TARGETS=(
  x86_64-unknown-linux-gnu
  x86_64-pc-windows-msvc
  aarch64-apple-darwin
  x86_64-apple-darwin
)

if ! command -v rustup >/dev/null 2>&1; then
  echo "rustup not found — install it, or run 'cargo clippy' per target yourself." >&2
  exit 1
fi

TC="$(rustup which cargo)"; TC="$(dirname "$TC")"
export PATH="$TC:$PATH"
export RUSTC="$TC/rustc"
CARGO="$TC/cargo"

status=0
for target in "${TARGETS[@]}"; do
  printf '\033[1;35m==> %s\033[0m\n' "$target"
  rustup target add "$target" >/dev/null 2>&1 || true

  # Windows resource embedding needs a resource compiler we don't have when
  # cross-compiling; skip just that step so the Rust source still gets checked.
  # (CI builds Windows natively and embeds for real.)
  win_res=""
  [[ "$target" == *windows* ]] && win_res="DDF_SKIP_WINRES=1"

  if ! env $win_res "$CARGO" clippy --target "$target" --all-targets -- -D warnings; then
    printf '\033[1;31m    FAILED: %s\033[0m\n' "$target"
    status=1
  fi
done

if [[ $status -eq 0 ]]; then
  printf '\033[1;32mAll targets clean.\033[0m\n'
else
  printf '\033[1;31mOne or more targets failed — see above.\033[0m\n'
fi
exit $status
