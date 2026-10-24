slint::include_modules!();

fn main() -> anyhow::Result<()> {
    let window = AppWindow::new()?;
    window.run()?;
    Ok(())
}
