fn main() {
    // Re-run build.rs if this env var changes (needed for cargo caching).
    println!("cargo:rerun-if-env-changed=FLOO_RELEASE_VERSION");

    // Prefer FLOO_RELEASE_VERSION (set by CI from the git tag) over Cargo.toml version.
    // This ensures the compiled binary reports the correct release version.
    let version = std::env::var("FLOO_RELEASE_VERSION")
        .unwrap_or_else(|_| env!("CARGO_PKG_VERSION").to_string());
    let version = version.strip_prefix('v').unwrap_or(&version);
    println!("cargo:rustc-env=FLOO_VERSION={version}");
}
