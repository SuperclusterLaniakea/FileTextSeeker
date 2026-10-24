fn main() {
    println!("cargo:rerun-if-changed=icon.ico");
    println!("cargo:rerun-if-changed=app.manifest");
    println!("cargo:rerun-if-changed=app.rc");
    #[cfg(windows)]
    {
        let _ = embed_resource::compile("app.rc", embed_resource::NONE).manifest_optional();
    }
}
