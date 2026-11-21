//! Windows only: embeds the app icon and version info (taken from Cargo.toml) in the exe.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../mpp-ide/assets/mathpp.ico");
    #[cfg(windows)]
    {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("../mpp-ide/assets/mathpp.ico");
        res.set("ProductName", "Math++");
        res.set("FileDescription", "Math++ command line");
        res.set("OriginalFilename", "mpp.exe");
        res.set("InternalName", "mpp.exe");
        res.set("CompanyName", "Benjamin Elmore");
        res.set(
            "LegalCopyright",
            "Copyright (c) Benjamin Elmore. MIT OR Apache-2.0",
        );
        if let Err(e) = res.compile() {
            // A missing resource compiler must not stop a plain `cargo build`.
            println!("cargo:warning=could not embed the Windows resources: {e}");
        }
    }
}
