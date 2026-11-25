fn main() {
    for file in [
        "assets/windows.rc",
        "assets/icon.ico",
        "LICENSE",
        "dist/notices.txt",
    ] {
        println!("cargo:rerun-if-changed={file}");
    }
    embed_resource::compile("assets/windows.rc", embed_resource::NONE)
        .manifest_required()
        .expect("无法嵌入应用图标和许可证");
}
