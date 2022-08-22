use fathom::{
    application::{Application, WindowConfig},
    color::Color,
    geometry::{Extent, Px},
    gui::{Center, Column, Fill, SizedBox},
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let tree = Center::new(Column::with_children(vec![
        SizedBox::new(
            Extent {
                width: Px(100),
                height: Px(100),
            },
            Fill::new(Color::RED),
        ),
        SizedBox::new(
            Extent {
                width: Px(200),
                height: Px(100),
            },
            Fill::new(Color::GREEN),
        ),
        SizedBox::new(
            Extent {
                width: Px(300),
                height: Px(100),
            },
            Fill::new(Color::BLUE),
        ),
    ]));

    Application::new()?.run(vec![WindowConfig {
        title: "Window #1",
        extent: None,
        widget_tree: Box::new(tree),
    }]);

    Ok(())
}

/*
Layout::Center {}
*/

// fn ui_builder(pool: &mut ContainerPool) -> ContainerId {
//     /*
//     pool.make(Container::Fill(Color::BLUE))
//         .add_child(|p| p.make(Container::LayoutCenter {})
//             .add_child(|p| p.make(Container::Box {
//                 extent: Extent {
//                     width: Px(100),
//                     height: Px(100),
//                 },
//             }
//         ))
//     );
//     */
//     /*
//     {
//         "type": "fill",
//         "color": "blue",
//         "children": {
//             "type": "layout_center",
//             "children": {
//                 "type": "box",
//                 "extent": {
//                     "width": 100,
//                     "height": 100,
//                 },
//             },
//         },
//     },
//     */
//     /*
//     <fill color="blue">
//         <layout_center>
//             <box extent="100 100"/>
//         </layout_center>
//     </fill>
//     */
//     /*
//     widget_tree!{
//         pool,
//         fill color=Color::BLUE on_click=|f| { f.color = random() } {
//             layout_center axis=xy {
//                 box extent="100 100"
//             }
//         }
//     };
//     */
//     /*
//     arena.make::<XSplitPanel>(|c| {
//         c.add_child::<FillColor>().on_click(|this, _| this.color = random());
//         c.add_child::<YSplitPanel>(|c| {
//             c.add_child::<FillColor>().on_click(|this, _|, this.color = random());
//             c.add_child::<FillColor>().on_click(|this, _| this.color = random());
//         });
//     })
//     */
//     /*
//     arena.make_auto::<XSplitPanel>([
//         arena.make::<FillColor>(),
//         arena.make_auto::<YSplitPanel>([
//             arena.make::<FillColor>(),
//             arena.make::<FillColor>(),
//         ]),
//     ])
//     */
//     /*
//     XSPlitPanel::auto([
//         FillColor::new(),
//         YSplitPanel::auto([
//             FillColor::new(),
//             FillColor::new(),
//         ]),
//     ])
//     */
// }
