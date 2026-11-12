//! Compiles the Swift shim (AppleScript objects and App Intents) into a static library
//! the executable links, then exposes the `angelly_shim` cfg to the Rust code.
//!
//! Without a Swift toolchain, or when not targeting macOS, the shim is skipped and the
//! Rust code compiles without it.

use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rustc-check-cfg=cfg(angelly_shim)");
    println!("cargo:rerun-if-changed=shim");
    println!("cargo:rerun-if-env-changed=MACOSX_DEPLOYMENT_TARGET");
    println!("cargo:rerun-if-env-changed=ANGELLY_SKIP_SHIM");

    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") || env::var_os("ANGELLY_SKIP_SHIM").is_some() {
        return;
    }
    if !cfg!(target_os = "macos") {
        println!("cargo:warning=the Swift shim can only be built on macOS; skipping it");
        return;
    }
    match build_shim() {
        Ok(()) => println!("cargo:rustc-cfg=angelly_shim"),
        Err(error) => println!("cargo:warning=Swift shim not built ({error}); AppleScript and Shortcuts are unavailable"),
    }
}

/// The protocols whose conforming types the App Intents extractor needs constant values for.
const INTENT_PROTOCOLS: &str = r#"["AppIntent","EntityQuery","AppEntity","TransientEntity","AppEnum","AppShortcutProviding","AppShortcutsProvider","AnyResolverProviding","AppIntentsPackage","DynamicOptionsProvider","_IntentValueRepresentable","_AssistantIntentsProvider","_GenerativeFunctionExtractable","IntentValueQuery","Resolver","AppExtension","ExtensionPointDefining"]"#;

/// Turns the compiler's constant values into `Metadata.appintents`, the bundle resource
/// Shortcuts reads to learn about the app's actions, and places it next to the final
/// executable (`target/<profile>`) for the bundle script to pick up.
fn extract_intents(
    out: &std::path::Path,
    sources: &[PathBuf],
    swiftc: &str,
    sdk: &str,
    deployment: &str,
    target: &str,
) -> Result<(), String> {
    let toolchain = PathBuf::from(swiftc)
        .ancestors()
        .nth(3)
        .map(PathBuf::from)
        .ok_or("cannot locate the toolchain")?;
    let xcode_version = run(Command::new("xcodebuild").arg("-version"))?
        .lines()
        .find_map(|line| line.strip_prefix("Build version ").map(str::to_owned))
        .ok_or("cannot read the Xcode build version")?;

    let list = |name: &str, lines: &[String]| -> Result<PathBuf, String> {
        let path = out.join(name);
        std::fs::write(&path, lines.join("\n")).map_err(|e| e.to_string())?;
        Ok(path)
    };
    let source_list = list("intent_sources.txt", &sources.iter().map(|p| p.display().to_string()).collect::<Vec<_>>())?;
    let const_values = list("intent_const_values.txt", &[out.join("AngellyShim.swiftconstvalues").display().to_string()])?;
    let empty = list("intent_empty.txt", &[])?;

    let metadata_dir = out.join("intents");
    let _ = std::fs::remove_dir_all(&metadata_dir);
    std::fs::create_dir_all(&metadata_dir).map_err(|e| e.to_string())?;
    let processor = run(Command::new("xcrun").args(["--find", "appintentsmetadataprocessor"]))?;
    run(Command::new(processor)
        .arg("--output")
        .arg(&metadata_dir)
        .arg("--toolchain-dir")
        .arg(&toolchain)
        .args(["--module-name", "AngellyShim", "--sdk-root", sdk, "--xcode-version", &xcode_version])
        .args(["--platform-family", "macOS", "--deployment-target", deployment, "--target-triple", target])
        .arg("--source-file-list")
        .arg(&source_list)
        .arg("--swift-const-vals-list")
        .arg(&const_values)
        .arg("--metadata-file-list")
        .arg(&empty)
        .arg("--static-metadata-file-list")
        .arg(&empty)
        .args(["--force", "--deployment-aware-processing", "--no-app-shortcuts-localization"]))?;

    let profile_dir = out.ancestors().nth(3).ok_or("cannot locate the target directory")?;
    let destination = profile_dir.join("Metadata.appintents");
    let _ = std::fs::remove_dir_all(&destination);
    run(Command::new("cp").arg("-R").arg(metadata_dir.join("Metadata.appintents")).arg(&destination))?;
    Ok(())
}

fn run(command: &mut Command) -> Result<String, String> {
    let output = command.output().map_err(|e| format!("{:?}: {e}", command.get_program()))?;
    if !output.status.success() {
        return Err(format!("{:?} failed: {}", command.get_program(), String::from_utf8_lossy(&output.stderr)));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn swift_target(deployment: &str) -> Result<String, String> {
    let arch = match env::var("CARGO_CFG_TARGET_ARCH").as_deref() {
        Ok("aarch64") => "arm64",
        Ok("x86_64") => "x86_64",
        other => return Err(format!("unsupported architecture {other:?}")),
    };
    Ok(format!("{arch}-apple-macos{deployment}"))
}

fn build_shim() -> Result<(), String> {
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let shim = manifest.join("shim");
    let out = PathBuf::from(env::var("OUT_DIR").unwrap());
    let deployment = env::var("MACOSX_DEPLOYMENT_TARGET").unwrap_or_else(|_| "14.0".into());
    let target = swift_target(&deployment)?;

    let sdk = run(Command::new("xcrun").args(["--sdk", "macosx", "--show-sdk-path"]))?;
    let swiftc = run(Command::new("xcrun").args(["--find", "swiftc"]))?;

    let mut sources: Vec<PathBuf> = std::fs::read_dir(&shim)
        .map_err(|e| e.to_string())?
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|path| path.extension().is_some_and(|ext| ext == "swift"))
        .collect();
    sources.sort();

    let object = out.join("angelly_shim.o");
    let protocols = out.join("const_extract_protocols.json");
    std::fs::write(&protocols, INTENT_PROTOCOLS).map_err(|e| e.to_string())?;
    let optimize = if env::var("PROFILE").as_deref() == Ok("release") { "-O" } else { "-Onone" };
    run(Command::new(&swiftc)
        .args(["-parse-as-library", "-swift-version", "5", "-wmo", optimize, "-module-name", "AngellyShim"])
        .args(["-target", &target, "-sdk", &sdk])
        .arg("-import-objc-header")
        .arg(shim.join("include/angelly_bridge.h"))
        .arg("-emit-object")
        .arg("-o")
        .arg(&object)
        .arg("-emit-const-values")
        .args(["-Xfrontend", "-const-gather-protocols-file", "-Xfrontend"])
        .arg(&protocols)
        .args(&sources))?;

    match extract_intents(&out, &sources, &swiftc, &sdk, &deployment, &target) {
        Ok(()) => {}
        Err(error) => println!("cargo:warning=App Intents metadata not produced ({error}); Shortcuts will not list the actions"),
    }

    let library = out.join("libangellyshim.a");
    let _ = std::fs::remove_file(&library);
    run(Command::new("ar").arg("rcs").arg(&library).arg(&object))?;

    println!("cargo:rustc-link-search=native={}", out.display());
    println!("cargo:rustc-link-lib=static=angellyshim");
    for dir in [
        PathBuf::from(&sdk).join("usr/lib/swift"),
        PathBuf::from("/usr/lib/swift"),
    ] {
        if dir.exists() {
            println!("cargo:rustc-link-search=native={}", dir.display());
        }
    }
    println!("cargo:rustc-link-arg=-Wl,-rpath,/usr/lib/swift");
    for framework in ["AppKit", "AppIntents", "Foundation"] {
        println!("cargo:rustc-link-lib=framework={framework}");
    }
    Ok(())
}
