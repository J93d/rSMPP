use std::path::Path;

fn main() -> std::io::Result<()> {
    slint_build::compile("ui/appwindow.slint")
        .expect("Failed to compile Slint UI — check ui/appwindow.slint for syntax errors");

    if cfg!(target_os = "windows") {
        let icon_png = Path::new("icon.png");
        let icon_ico = Path::new("icon.ico");

        // Convert PNG to ICO if PNG exists and ICO doesn't (or force update logic if needed)
        // For simplicity: If png exists, try to generate ico.
        if icon_png.exists() {
            match image::open(icon_png) {
                Ok(img) => {
                    // Start resize if too large
                    let resized = img.resize(256, 256, image::imageops::FilterType::Lanczos3);

                    if let Err(e) = resized.save(icon_ico) {
                        println!("cargo:warning=Failed to save icon.ico: {}", e);
                    } else {
                        // Success
                        println!("cargo:rerun-if-changed=icon.png");
                    }
                }
                Err(e) => {
                    println!("cargo:warning=Failed to open icon.png: {}", e);
                }
            }
        }

        let mut res = winres::WindowsResource::new();
        res.set("FileDescription", "rSMPP Client");
        res.set("ProductName", "rSMPP");
        res.set("OriginalFilename", "rSMPP.exe");
        res.set("LegalCopyright", "Copyright (c) 2024");
        res.set("CompanyName", "My Company"); // Replace with your company name

        if icon_ico.exists() {
            if let Some(path_str) = icon_ico.to_str() {
                res.set_icon(path_str);
            } else {
                println!("cargo:warning=Icon path contains non-UTF8 characters, skipping");
            }
        }

        res.compile()?;
    }

    Ok(())
}
