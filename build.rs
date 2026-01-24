fn main() -> std::io::Result<()> {
    slint_build::compile("ui/appwindow.slint").unwrap();
    
    if cfg!(target_os = "windows") {
        let mut res = winres::WindowsResource::new();
        res.set("FileDescription", "rSMPP Client");
        res.set("ProductName", "rSMPP");
        res.set("OriginalFilename", "rSMPP.exe");
        res.set("LegalCopyright", "Copyright (c) 2024");
        res.set_icon("icon.ico"); // If you have an icon
        res.compile()?;
    }
    
    Ok(())
}