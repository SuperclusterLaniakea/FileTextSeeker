fn main() {
    if cfg!(target_os = "windows") {
        let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
        let manifest_path = std::path::Path::new(&manifest_dir);

        // 编译资源文件（嵌入图标到 exe）
        let rc_path = manifest_path.join("resource.rc");
        embed_resource::compile(&rc_path, embed_resource::NONE);

        // 将 icon.ico 复制到输出目录，确保运行时能找到
        let icon_src = manifest_path.join("icon.ico");
        if icon_src.exists() {
            // OUT_DIR = target/debug/build/<pkg>-<hash>/out/
            let out_dir = std::env::var("OUT_DIR").unwrap();
            let out_path = std::path::Path::new(&out_dir);
            // 目标：target/debug/ 或 target/release/
            let build_dir = out_path.ancestors().nth(3).unwrap();
            let icon_dst = build_dir.join("icon.ico");
            let _ = std::fs::copy(&icon_src, &icon_dst);
            println!("cargo:rerun-if-changed={}", icon_src.display());
        }
    }
}