fn main() {
    println!("cargo:rerun-if-changed=assets/icon/v-autoclicker.ico");

    // `cfg(windows)` in a build script describes the host that runs the
    // script, so inspect the actual compilation target instead. This keeps
    // the icon in the executable when cross-compiling from Linux.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    let mut resource = winresource::WindowsResource::new();
    resource.set_icon("assets/icon/v-autoclicker.ico");
    resource.set("ProductName", "V-AutoClicker");
    resource.set("FileDescription", "V-AutoClicker");
    resource.set("OriginalFilename", "v-autoclicker.exe");

    // Cross-compilation hosts may not have a resource compiler installed.
    // A missing icon must not break the rest of the build.
    if let Err(error) = resource.compile() {
        println!("cargo:warning=failed to embed the Windows icon: {error}");
    }
}
