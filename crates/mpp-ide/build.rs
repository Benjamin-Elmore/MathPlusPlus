//! Windows only: embeds the app icon and version info (taken from Cargo.toml) in the exe,
//! and gives the main thread a large stack (other platforms need none; see below).

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    // A large main-thread stack (reserved, not committed), Windows only: Windows gives a
    // program 1 MB unless the linker says more, while Linux and macOS give the main
    // thread 8 MB, which is enough. The parser runs deep input on its own big-stack
    // thread, and crates/mpp-render/tests/stack.rs lays out the deepest maths the
    // parser accepts on an 8 MB stack, so the other platforms need no flag.
    let os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let env = std::env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    if os == "windows" && env == "msvc" {
        println!("cargo:rustc-link-arg-bins=/STACK:268435456");
    } else if os == "windows" && env == "gnu" {
        println!("cargo:rustc-link-arg-bins=-Wl,--stack,268435456");
    }
    println!("cargo:rerun-if-changed=../mpp-ide/assets/mathpp.ico");
    #[cfg(windows)]
    {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("../mpp-ide/assets/mathpp.ico");
        res.set("ProductName", "Math++");
        res.set("FileDescription", "Math++ IDE");
        res.set("OriginalFilename", "Math++.exe");
        res.set("InternalName", "Math++.exe");
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
