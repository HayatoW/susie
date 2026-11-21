fn main() {
    let cef_dir = std::env::var("DEP_SUSIE_ENGINE_CEF_DIR").unwrap_or_else(|_| {
        panic!("engine が CEF の場所を渡していません");
    });
    println!("cargo:rustc-link-arg=-Wl,-rpath,{cef_dir}");
}
