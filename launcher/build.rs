fn main() {
    println!("cargo:rerun-if-changed=../src-tauri/icons/icon.ico");
    #[cfg(windows)]
    {
        let mut res = tauri_winres::WindowsResource::new();
        res.set_icon("../src-tauri/icons/icon.ico");
        res.set("ProductName", "CPAH Docs");
        res.set("FileDescription", "CPAH Docs Launcher");
        res.compile().expect("failed to compile Windows resource");
    }
}
