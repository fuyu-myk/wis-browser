use std::env;
use std::path::PathBuf;
use std::process::Command;

/// Lowest macOS Wis supports (`bundle.macOS.minimumSystemVersion`), used when
/// the Tauri CLI has not set `MACOSX_DEPLOYMENT_TARGET`.
const DEFAULT_MACOS_TARGET: &str = "14.0";

const TRANSLATION_BRIDGE: &str = "swift/Translation.swift";

fn main() {
    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        // Touch ID / biometric unlock for the password vault.
        println!("cargo:rustc-link-lib=framework=LocalAuthentication");
        // Language detection for page translation.
        println!("cargo:rustc-link-lib=framework=NaturalLanguage");
        build_translation_bridge();
    }
    tauri_build::build()
}

fn xcrun(args: &[&str]) -> String {
    let out = Command::new("xcrun")
        .args(args)
        .output()
        .unwrap_or_else(|err| panic!("xcrun {args:?}: {err}"));
    assert!(
        out.status.success(),
        "xcrun {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr).trim()
    );
    String::from_utf8(out.stdout)
        .expect("xcrun prints UTF-8")
        .trim()
        .to_string()
}

/// Apple's Swift-only Translation framework, compiled into a static
/// library. The framework is weak-linked so Wis still launches on
/// macOS releases that lack it.
fn build_translation_bridge() {
    println!("cargo:rerun-if-changed={TRANSLATION_BRIDGE}");
    for var in [
        "MACOSX_DEPLOYMENT_TARGET",
        "DEVELOPER_DIR",
        "SDKROOT",
        "TOOLCHAINS",
    ] {
        println!("cargo:rerun-if-env-changed={var}");
    }
    let out = PathBuf::from(env::var("OUT_DIR").expect("cargo sets OUT_DIR"));
    let arch = match env::var("CARGO_CFG_TARGET_ARCH").as_deref() {
        Ok("aarch64") => "arm64",
        Ok("x86_64") => "x86_64",
        other => panic!("no Swift target for {other:?}"),
    };
    let deployment =
        env::var("MACOSX_DEPLOYMENT_TARGET").unwrap_or_else(|_| DEFAULT_MACOS_TARGET.into());
    let sdk = xcrun(&["--sdk", "macosx", "--show-sdk-path"]);
    let status = Command::new("xcrun")
        .args(["--sdk", "macosx", "swiftc", "-emit-library", "-static"])
        .args([
            "-parse-as-library",
            "-swift-version",
            "6",
            "-O",
            "-whole-module-optimization",
        ])
        .args(["-module-name", "WisTranslation", "-sdk", &sdk])
        .args(["-target", &format!("{arch}-apple-macos{deployment}")])
        .arg(TRANSLATION_BRIDGE)
        .arg("-o")
        .arg(out.join("libWisTranslation.a"))
        .status()
        .expect("swiftc runs");
    assert!(status.success(), "swiftc failed on {TRANSLATION_BRIDGE}");
    println!("cargo:rustc-link-search=native={}", out.display());
    println!("cargo:rustc-link-lib=static=WisTranslation");

    // The Swift runtime ships with macOS. A build whose deployment target
    // predates it records some of it by `@rpath` name, which this rpath
    // resolves to the system copy.
    println!("cargo:rustc-link-search=native={sdk}/usr/lib/swift");
    println!("cargo:rustc-link-arg=-Wl,-rpath,/usr/lib/swift");
    println!("cargo:rustc-link-arg=-Wl,-weak_framework,Translation");
}
