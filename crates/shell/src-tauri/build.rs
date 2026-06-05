fn main() {
    // Touch ID / biometric unlock for the password vault.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        println!("cargo:rustc-link-lib=framework=LocalAuthentication");
    }
    tauri_build::build()
}
