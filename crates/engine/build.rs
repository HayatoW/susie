fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=CEF_PATH");

    let cef_dir = std::env::var("DEP_CEF_DLL_WRAPPER_CEF_DIR").unwrap_or_else(|_| {
        panic!("cef-dll-sys did not export CEF_DIR");
    });
    println!("cargo:rustc-env=SUSIE_CEF_DIR={cef_dir}");
    println!("cargo:rustc-link-arg=-Wl,-rpath,{cef_dir}");
    println!("cargo:cef_dir={cef_dir}");
}
