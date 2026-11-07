fn main() {
    println!("cargo:rerun-if-changed=native/macos.m");
    println!("cargo:rerun-if-changed=native/Info.plist");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        let mut build = cc::Build::new();
        build
            .file("native/macos.m")
            .flag("-fobjc-arc")
            .flag("-fblocks");
        if std::env::var_os("CARGO_FEATURE_TEST_TRANSPORT").is_some() {
            build.define("BLUGIT_TEST_TRANSPORT", None);
        }
        build.compile("blugit_macos");
        let frameworks = [
            "Foundation",
            "CoreFoundation",
            "IOBluetooth",
            "CoreBluetooth",
            "AppKit",
        ];
        // Keep the permission owner's code signature independent of Rust/Git
        // changes and Cargo profiles. Only native helper changes require macOS
        // to evaluate a new executable identity.
        let helper =
            std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap()).join("blugit-bluetooth");
        let mut command = cc::Build::new()
            .opt_level(2)
            .debug(false)
            .get_compiler()
            .to_command();
        command
            .args([
                "-x",
                "objective-c",
                "-fobjc-arc",
                "-fblocks",
                "-O2",
                "-g0",
                "-fno-omit-frame-pointer",
                "-DBLUGIT_STANDALONE_HELPER",
                "native/macos.m",
                "-o",
            ])
            .arg(&helper);
        for framework in frameworks {
            command.args(["-framework", framework]);
            println!("cargo:rustc-link-lib=framework={framework}");
        }
        let status = command.status().expect("launch macOS helper compiler");
        assert!(
            status.success(),
            "compile standalone macOS Bluetooth helper"
        );
    }
}
