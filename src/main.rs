use fathom::application::{Application, WindowConfig};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    Application::new()?.run(&[WindowConfig {
        title: "Window #1",
        extent: None,
        ui_builder: &ui_builder,
    }]);
    Ok(())
}

fn ui_builder() {}
