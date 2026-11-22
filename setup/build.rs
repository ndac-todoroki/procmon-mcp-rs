//! Embeds the procmon-mcp.exe built in the same profile, which the setup installs.
//!
//! Build it first: `cargo build --release -p procmon-mcp`, then
//! `cargo build --release -p procmon-mcp-setup`. Cargo cannot express a dependency on
//! another package's binary on stable, so the order is up to the caller.

use std::path::PathBuf;

fn main() {
    embed_manifest();
    embed_server();
}

/// Embeds setup.manifest through the MSVC linker, so no resource compiler or extra crate
/// is needed. /MANIFESTUAC:NO keeps the linker from adding a second trustInfo.
fn embed_manifest() {
    let manifest = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap()).join("setup.manifest");
    println!("cargo:rerun-if-changed={}", manifest.display());
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() != Ok("msvc") {
        println!("cargo:warning=not an MSVC target; the setup is built without its manifest");
        return;
    }
    println!("cargo:rustc-link-arg-bins=/MANIFEST:EMBED");
    println!("cargo:rustc-link-arg-bins=/MANIFESTINPUT:{}", manifest.display());
    println!("cargo:rustc-link-arg-bins=/MANIFESTUAC:NO");
}

fn embed_server() {
    let out = PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    // OUT_DIR lies under <target>/<profile>/build/ (how deep depends on the Cargo version).
    let profile_dir = out
        .ancestors()
        .find(|dir| dir.file_name().is_some_and(|name| name == "build"))
        .and_then(|build| build.parent())
        .expect("OUT_DIR is inside the profile's build directory")
        .to_path_buf();
    let server = profile_dir.join("procmon-mcp.exe");
    let embedded = out.join("procmon-mcp.exe");
    println!("cargo:rerun-if-changed={}", server.display());
    let release = std::env::var("PROFILE").as_deref() == Ok("release");
    match std::fs::copy(&server, &embedded) {
        Ok(_) => {}
        Err(e) if release => panic!(
            "cannot embed {}: {e}; run `cargo build --release -p procmon-mcp` first",
            server.display()
        ),
        Err(_) => {
            // Lets `cargo clippy` and `cargo test` work on a fresh checkout. The setup
            // refuses to install from an empty payload.
            println!("cargo:warning=procmon-mcp.exe not built yet; embedding an empty payload");
            std::fs::write(&embedded, []).unwrap();
        }
    }
}
