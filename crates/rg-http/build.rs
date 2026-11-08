fn main() {
    // Only report provenance supplied by the build system. A checkout's HEAD
    // alone cannot prove which modified files were included in the artifact.
    println!("cargo:rerun-if-env-changed=IRONFORGE_BUILD_COMMIT");
    println!("cargo:rerun-if-env-changed=IRONFORGE_BUILD_DIRTY");
    let commit = std::env::var("IRONFORGE_BUILD_COMMIT").unwrap_or_default();
    let dirty = std::env::var("IRONFORGE_BUILD_DIRTY").unwrap_or_default();
    let valid_commit =
        matches!(commit.len(), 40 | 64) && commit.bytes().all(|b| b.is_ascii_hexdigit());
    println!(
        "cargo:rustc-env=IRONFORGE_BUILD_COMMIT={}",
        if valid_commit { &commit } else { "" }
    );
    println!(
        "cargo:rustc-env=IRONFORGE_BUILD_DIRTY={}",
        if matches!(dirty.as_str(), "true" | "false") {
            &dirty
        } else {
            ""
        }
    );
}
