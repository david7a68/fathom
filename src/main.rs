use fathom::{
    application::{Application, WindowDesc},
    geometry::{Extent, Px},
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    Application::new()?.run(&[WindowDesc {
        title: "Hello, world!".to_string(),
        size: Extent {
            width: Px(800),
            height: Px(600),
        },
    }]);
    Ok(())
}
