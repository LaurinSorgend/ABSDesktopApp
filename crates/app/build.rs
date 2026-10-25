//! Compiles the Slint UI and, for Windows targets, embeds the app icon in the exe.

fn main() {
    slint_build::compile("ui/app.slint").expect("compile the Slint UI");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        embed_windows_icon();
    }
}

fn embed_windows_icon() {
    const ICON: &str = "../../packaging/icons/abs-desktop.ico";
    println!("cargo:rerun-if-changed={ICON}");
    winresource::WindowsResource::new()
        .set_icon(ICON)
        .compile()
        .expect("embed the Windows icon");
}
