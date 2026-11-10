//! Embed the app icon and version metadata into QuickSwitch.exe (plan §10.1 step 3).

fn main() {
    println!("cargo:rerun-if-changed=resources/quickswitch.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("resources/quickswitch.ico")
            .set("ProductName", "QuickSwitch")
            .set("FileDescription", "QuickSwitch window and folder switcher")
            .set("CompanyName", "QuickSwitch")
            .set("OriginalFilename", "QuickSwitch.exe")
            .set("LegalCopyright", "QuickSwitch contributors");
        if let Err(e) = res.compile() {
            // Missing Windows SDK resource compiler: build without the icon
            // rather than failing outright.
            println!("cargo:warning=could not embed Windows resources: {e}");
        }
    }
}
