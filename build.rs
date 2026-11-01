fn main() {
    println!("cargo:rerun-if-env-changed=VELDIR_BUILD_COMMIT");
    let commit = std::env::var("VELDIR_BUILD_COMMIT").unwrap_or_else(|_| "local".into());
    println!("cargo:rustc-env=VELDIR_BUILD_COMMIT={commit}");
}
