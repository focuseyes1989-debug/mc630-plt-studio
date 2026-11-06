fn main() {
    println!("cargo:rerun-if-changed=assets/studio-icon.ico");
    #[cfg(windows)]
    {
        let mut resource = winresource::WindowsResource::new();
        resource.set_icon("assets/studio-icon.ico");
        resource.compile().expect("embed the Windows app icon");
    }
}
