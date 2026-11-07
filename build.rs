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
        for framework in [
            "Foundation",
            "CoreFoundation",
            "IOBluetooth",
            "CoreBluetooth",
        ] {
            println!("cargo:rustc-link-lib=framework={framework}");
        }
    }
}
