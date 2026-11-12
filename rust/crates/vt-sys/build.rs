use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

fn main() {
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let repo = manifest.join("../../..").canonicalize().unwrap();
    let include = repo.join("include");

    println!("cargo:rerun-if-env-changed=GHOSTTY_VT_PREFIX");
    println!("cargo:rerun-if-env-changed=GHOSTTY_VT_OPTIMIZE");
    println!("cargo:rerun-if-env-changed=ZIG");
    println!("cargo:rerun-if-changed={}", include.display());

    let prefix = match env::var_os("GHOSTTY_VT_PREFIX") {
        Some(p) => PathBuf::from(p),
        None => {
            for p in ["src", "build.zig", "build.zig.zon"] {
                println!("cargo:rerun-if-changed={}", repo.join(p).display());
            }
            build_with_zig(&repo)
        }
    };

    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap();
    let target_env = env::var("CARGO_CFG_TARGET_ENV").unwrap();
    let lib = prefix.join("lib");
    if target_os == "windows" {
        // Zig names the static archive `ghostty-vt-static.lib`; the DLL's import library
        // already owns `ghostty-vt.lib`. A GNU linker only looks for `lib<name>.a`.
        let archive = lib.join("ghostty-vt-static.lib");
        assert!(archive.exists(), "ghostty-vt-static.lib not found in {}", lib.display());
        if target_env == "gnu" {
            let out = PathBuf::from(env::var("OUT_DIR").unwrap());
            fs::copy(&archive, out.join("libghostty-vt-static.a")).unwrap();
            println!("cargo:rustc-link-search=native={}", out.display());
        } else {
            println!("cargo:rustc-link-search=native={}", lib.display());
        }
        println!("cargo:rustc-link-lib=static=ghostty-vt-static");
    } else {
        assert!(
            lib.join("libghostty-vt.a").exists(),
            "libghostty-vt.a not found in {}",
            lib.display()
        );
        println!("cargo:rustc-link-search=native={}", lib.display());
        println!("cargo:rustc-link-lib=static=ghostty-vt");
        let cxx = if target_os == "macos" { "c++" } else { "stdc++" };
        println!("cargo:rustc-link-lib=dylib={cxx}");
    }

    let bindings = bindgen::Builder::default()
        .header(include.join("ghostty/vt.h").to_str().unwrap())
        .clang_arg(format!("-I{}", include.display()))
        .allowlist_function("ghostty_.*")
        .allowlist_type("Ghostty.*")
        .allowlist_var("GHOSTTY_.*")
        .default_enum_style(bindgen::EnumVariation::ModuleConsts)
        .derive_default(true)
        .generate()
        .expect("bindgen failed");
    bindings
        .write_to_file(PathBuf::from(env::var("OUT_DIR").unwrap()).join("bindings.rs"))
        .unwrap();
}

/// Builds libghostty-vt with zig into `<cargo target dir>/ghostty-vt/<triple>`
/// and returns the install prefix. Zig's own cache makes repeat runs cheap.
fn build_with_zig(repo: &Path) -> PathBuf {
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    // OUT_DIR is <target>/<profile>/build/vt-sys-<hash>/out
    let target_dir = out_dir
        .ancestors()
        .nth(4)
        .expect("unexpected OUT_DIR layout");
    let triple = env::var("TARGET").unwrap();
    let root = target_dir.join("ghostty-vt").join(&triple);
    let prefix = root.join("prefix");
    fs::create_dir_all(&root).unwrap();

    let optimize = env::var("GHOSTTY_VT_OPTIMIZE").unwrap_or_else(|_| "ReleaseFast".into());
    let zig = env::var("ZIG").unwrap_or_else(|_| "zig".into());
    let mut cmd = Command::new(&zig);
    cmd.current_dir(repo)
        .args(["build", "-Demit-lib-vt", &format!("-Doptimize={optimize}")])
        .arg("--prefix")
        .arg(&prefix)
        .arg("--cache-dir")
        .arg(root.join("zig-cache"));
    if triple != env::var("HOST").unwrap() {
        cmd.arg(format!("-Dtarget={}", zig_target(&triple)));
    }

    let status = cmd.status().unwrap_or_else(|e| {
        panic!("failed to run `{zig} build` ({e}); install zig or set GHOSTTY_VT_PREFIX")
    });
    assert!(status.success(), "zig build -Demit-lib-vt failed");
    prefix
}

fn zig_target(rust_triple: &str) -> String {
    let arch = rust_triple.split('-').next().unwrap();
    let os = match rust_triple {
        t if t.contains("apple-darwin") => "macos",
        t if t.contains("linux-musl") => "linux-musl",
        t if t.contains("linux-gnu") => "linux-gnu",
        t if t.ends_with("windows-gnu") => "windows-gnu",
        t if t.ends_with("windows-msvc") => "windows-msvc",
        t => panic!("unsupported target {t}; set GHOSTTY_VT_PREFIX"),
    };
    format!("{arch}-{os}")
}
