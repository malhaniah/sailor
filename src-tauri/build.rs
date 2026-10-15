fn main() {
    // Windows embedded player: link against the vendored libmpv import library.
    // `scripts/fetch-libmpv.mjs` populates src-tauri/libmpv/ (hash-pinned DLL +
    // mpv.lib generated from mpv.def). libmpv-2.dll is linked DELAYLOAD so the
    // app still starts when the DLL is missing — the runtime probes for it and
    // falls back to the external mpv backend (see commands/mpv.rs).
    #[cfg(windows)]
    if std::env::var_os("CARGO_FEATURE_EMBEDDED").is_some() {
        let libmpv = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("libmpv");
        println!("cargo:rerun-if-changed={}", libmpv.display());
        println!("cargo:rerun-if-env-changed=SAILOR_SKIP_LIBMPV_CHECK");
        // escape hatch for `cargo check`/clippy runs that never link
        let skip_check = std::env::var_os("SAILOR_SKIP_LIBMPV_CHECK").is_some();
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

    // tauri.embedded.conf.json (passed via --config for embedded builds) maps
    // libmpv/libmpv-2.dll into the bundle, and tauri_build refuses to compile
    // when a mapped resource is missing. Drop in a placeholder so such builds
    // compile before `node scripts/fetch-libmpv.mjs` has run (the script
    // replaces it; the runtime probe rejects it via LoadLibrary).
    #[cfg(windows)]
    {
        let dll = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("libmpv")
            .join("libmpv-2.dll");
        if !dll.exists() {
            let _ = std::fs::create_dir_all(dll.parent().unwrap());
            let _ = std::fs::write(
                &dll,
                "placeholder - run `node scripts/fetch-libmpv.mjs` to vendor the real libmpv\n",
            );
        }
    }

    tauri_build::build()
}
