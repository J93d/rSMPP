fn main() -> std::io::Result<()> {
    slint_build::compile("ui/appwindow.slint").unwrap();
    Ok(())
}