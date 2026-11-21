//! Embeds the procmon-mcp.exe built in the same profile, which the setup installs.
//!
//! Build it first: `cargo build --release -p procmon-mcp`, then
//! `cargo build --release -p procmon-mcp-setup`. Cargo cannot express a dependency on
//! another package's binary on stable, so the order is up to the caller.

use std::path::PathBuf;

fn main() {
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
