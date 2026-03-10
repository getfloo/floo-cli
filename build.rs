fn main() {
    println!("cargo:rustc-env=FLOO_VERSION={}", env!("CARGO_PKG_VERSION"));
}
