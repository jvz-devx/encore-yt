//! Windows: embeds the app icon as resource 1, the id GPUI loads for the
//! window's title bar, the taskbar and Alt-Tab. Other targets need nothing.

use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../../packaging/icons/encore-yt.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let manifest = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap_or_default());
    let out = PathBuf::from(std::env::var_os("OUT_DIR").unwrap_or_default());
    let icon = manifest.join("../../packaging/icons/encore-yt.ico");
    let icon = icon.canonicalize().unwrap_or(icon);
    // A resource script with the icon's absolute path, so it doesn't depend
    // on where the resource compiler runs.
    let script = out.join("encore-yt.rc");
    let path = icon.display().to_string().replace('\\', "\\\\");
    if let Err(error) = std::fs::write(&script, format!("1 ICON \"{path}\"\n")) {
        panic!("couldn't write {}: {error}", script.display());
    }
    if let Err(error) = embed_resource::compile(&script, embed_resource::NONE).manifest_optional() {
        panic!("couldn't embed the app icon: {error}");
    }
}
