use fathom::application::{Application, WindowConfig};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    Application::new()?.run(&[WindowConfig {
        title: "Window #1",
        extent: None,
    }]);
    Ok(())
}
