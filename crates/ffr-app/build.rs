//! Windows resources (icon, version info, manifest) and the release-build flag.

use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-env-changed=FFR_RELEASE_BUILD");
    println!("cargo:rerun-if-changed=ffr.rc");
    println!("cargo:rerun-if-changed=ffr.manifest");
    println!("cargo:rerun-if-changed=../../assets/icons/icon.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let manifest_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default());
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap_or_default());
    let version = std::env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "0.0.0".into());
    let core = version.split('-').next().unwrap_or("0.0.0");
    let mut parts: Vec<u16> = core.split('.').map(|x| x.parse().unwrap_or(0)).collect();
    parts.resize(4, 0);
    let commas = parts
        .iter()
        .map(|p| p.to_string())
        .collect::<Vec<_>>()
        .join(",");
    // Debug builds use asInvoker so `cargo run` works from a normal terminal; release builds
    // ask for administrator rights once at launch.
    let profile = std::env::var("PROFILE").unwrap_or_default();
    let level = if profile == "release" {
        "requireAdministrator"
    } else {
        "asInvoker"
    };
    let manifest = std::fs::read_to_string(manifest_dir.join("ffr.manifest"))
        .unwrap_or_default()
        .replace("@LEVEL@", level)
        .replace(
            "@VERSION@",
            &format!("{}.{}.{}.{}", parts[0], parts[1], parts[2], parts[3]),
        );
    let manifest_out = out.join("ffr.manifest");
    std::fs::write(&manifest_out, manifest).expect("write manifest");
    let icon = manifest_dir.join("../../assets/icons/icon.ico");
    let rc = std::fs::read_to_string(manifest_dir.join("ffr.rc"))
        .unwrap_or_default()
        .replace("@ICON@", &icon.display().to_string().replace('\\', "/"))
        .replace(
            "@MANIFEST@",
            &manifest_out.display().to_string().replace('\\', "/"),
        )
        .replace("@VERSION_COMMAS@", &commas)
        .replace("@VERSION@", &version);
    let rc_out = out.join("ffr.rc");
    std::fs::write(&rc_out, rc).expect("write rc");
    embed_resource::compile(&rc_out, embed_resource::NONE)
        .manifest_required()
        .expect("compile Windows resources");
}
