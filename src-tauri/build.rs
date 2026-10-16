fn main() {
    // Windows embedded player: link against the vendored libmpv import library.
    // `scripts/fetch-libmpv.mjs` populates src-tauri/libmpv/ (hash-pinned DLL +
    // mpv.lib generated from mpv.def). libmpv-2.dll is linked DELAYLOAD so the
    // app still starts when the DLL is missing — the runtime probes for it and
    // falls back to the external mpv backend (see commands/mpv/windows.rs).
    #[cfg(windows)]
    if std::env::var_os("CARGO_FEATURE_EMBEDDED").is_some() {
        let libmpv = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("libmpv");
        println!("cargo:rerun-if-changed={}", libmpv.display());
        println!("cargo:rerun-if-env-changed=ZEILER_SKIP_LIBMPV_CHECK");
        // escape hatch for `cargo check`/clippy runs that never link
        let skip_check = std::env::var_os("ZEILER_SKIP_LIBMPV_CHECK").is_some();
        if !skip_check && !libmpv.join("mpv.lib").exists() {
            panic!(
                "building with `embedded` but {} is missing.\n\
                 Run `node scripts/fetch-libmpv.mjs` first (downloads the pinned \
                 libmpv-2.dll and generates mpv.lib).",
                libmpv.join("mpv.lib").display()
            );
        }
        println!("cargo:rustc-link-search=native={}", libmpv.display());
        println!("cargo:rustc-link-arg=/DELAYLOAD:libmpv-2.dll");
        println!("cargo:rustc-link-arg=delayimp.lib");

        // Convenience for `tauri dev`: put the DLL beside the dev executable
        // (OUT_DIR = target/<profile>/build/<pkg>/out).
        let dll = libmpv.join("libmpv-2.dll");
        if dll.exists()
            && let Some(profile_dir) = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap())
                .ancestors()
                .nth(3)
                .map(std::path::Path::to_path_buf)
        {
            let _ = std::fs::copy(&dll, profile_dir.join("libmpv-2.dll"));
        }
    }

    // macOS embedded player: link the vendored libmpv bundle for the target's
    // architecture (`node scripts/fetch-libmpv.mjs` puts it in src-tauri/libmpv-macos/
    // <arch>/). Its libraries are @rpath-named; the app bundle carries them in
    // Contents/Frameworks (the generated tauri.conf.json beside them), and tauri_build
    // adds the rpath @executable_path/../Frameworks for bundle.macOS.frameworks. Debug
    // builds (`tauri dev`, tests) also look in the vendor dir.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos")
        && std::env::var_os("CARGO_FEATURE_EMBEDDED").is_some()
    {
        let arch = match std::env::var("CARGO_CFG_TARGET_ARCH").as_deref() {
            Ok("aarch64") => "arm64",
            _ => "x86_64",
        };
        let lib = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("libmpv-macos")
            .join(arch)
            .join("lib");
        println!("cargo:rerun-if-changed={}", lib.display());
        println!("cargo:rerun-if-env-changed=ZEILER_SKIP_LIBMPV_CHECK");
        let skip_check = std::env::var_os("ZEILER_SKIP_LIBMPV_CHECK").is_some();
        if !skip_check && !lib.join("libmpv.2.dylib").exists() {
            panic!(
                "building with `embedded` but {} is missing.\n\
                 Run `node scripts/fetch-libmpv.mjs` first (downloads the pinned macOS \
                 libmpv bundle for {arch}).",
                lib.join("libmpv.2.dylib").display()
            );
        }
        println!("cargo:rustc-link-search=native={}", lib.display());
        if std::env::var("PROFILE").as_deref() == Ok("debug") {
            println!("cargo:rustc-link-arg=-Wl,-rpath,{}", lib.display());
        }
    }

    // tauri.embedded.conf.json (passed via --config for embedded builds) maps
    // libmpv/libmpv-2.dll and libmpv/NOTICE.txt into the bundle, and tauri_build
    // refuses to compile when a mapped resource is missing. Drop in placeholders
    // so such builds compile before `node scripts/fetch-libmpv.mjs` has run (the
    // script replaces both — a NOTICE.txt without the pinned hash does not count
    // as vendored; the runtime probe rejects the DLL via LoadLibrary).
    #[cfg(windows)]
    {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("libmpv");
        for file in ["libmpv-2.dll", "NOTICE.txt"] {
            let path = dir.join(file);
            if !path.exists() {
                let _ = std::fs::create_dir_all(&dir);
                let _ = std::fs::write(
                    &path,
                    "placeholder - run `node scripts/fetch-libmpv.mjs` to vendor the real libmpv\n",
                );
            }
        }
    }

    // tauri.conf.json bundles licenses/THIRD-PARTY-LICENSES.txt on every platform. Only
    // release builds generate it (`node scripts/third-party-licenses.mjs`, per target, in
    // release.yml) and it is git-ignored, so drop in a placeholder for every other build.
    let third_party = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("licenses")
        .join("THIRD-PARTY-LICENSES.txt");
    if !third_party.exists() {
        let _ = std::fs::write(
            &third_party,
            "placeholder - run `node scripts/third-party-licenses.mjs` to list the licences \
             of the third-party code in this build; see also THIRD-PARTY-NOTICES.md at \
             https://github.com/malhaniah/zeiler\n",
        );
    }

    tauri_build::build()
}
