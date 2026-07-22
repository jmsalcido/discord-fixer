//! Embeds the icon and application manifest into the Windows executable, so
//! Explorer shows a real icon and Windows knows the app is DPI-aware and needs
//! no elevation.

fn main() {
    println!("cargo:rerun-if-changed=packaging/windows/app.rc");
    println!("cargo:rerun-if-changed=packaging/windows/app.manifest");
    println!("cargo:rerun-if-changed=assets/icon.ico");

    // Keyed off the *target*, not the host — this must also fire when
    // cross-compiling to Windows.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        embed_resource::compile("packaging/windows/app.rc", embed_resource::NONE)
            .manifest_required()
            .expect("failed to embed Windows resources");
    }
}
