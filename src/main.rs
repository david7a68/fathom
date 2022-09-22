use fathom::{
    application::{Application, AppWindowConfig},
    gfx::color::Color,
    gui::widgets::{
        debug::Fill,
        split_panel::{Axis, SplitPanel},
        tabbed_panel::TabbedPanel,
        Widget,
    },
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let tree = SplitPanel::<Box<dyn Widget>>::with_children(
        Axis::X,
        vec![
            Box::new(Fill::new(Color::GREEN)),
            Box::new(SplitPanel::with_children(
                Axis::Y,
                vec![
                    TabbedPanel::with_children(vec![
                        Fill::new(Color::RED),
                        Fill::new(Color::BLUE),
                        Fill::new(Color::WHITE),
                    ]),
                    TabbedPanel::with_children(vec![
                        Fill::new(Color::RED),
                        Fill::new(Color::BLUE),
                        Fill::new(Color::WHITE),
                    ]),
                ],
            )),
            Box::new(Fill::new(Color::WHITE)),
        ],
    );

    Application::new()?.run(vec![AppWindowConfig {
        title: "Window #1",
        extent: None,
        widget_tree: Box::new(tree),
    }]);

    Ok(())
}
