use crate::{
    draw_command::DrawCommand,
    geometry::{Extent, Point, Px, Rect},
    indexed_tree::NodeList,
};

use super::{widget::Widget, Input, Layout, LayoutConstraint, LayoutTree};

pub struct XSplitPanel {
    pub panes: Vec<(f32, Box<dyn Widget>)>,
}

impl Widget for XSplitPanel {
    fn update(&self, input: &Input) {
        for pane in &self.panes {
            pane.1.update(input);
        }
    }

    fn layout<'a>(
        &'a self,
        constraints: LayoutConstraint,
        layout_tree: &mut LayoutTree<'a>,
    ) -> (NodeList<Layout<'a>>, Extent) {
        let mut moving_x = Px(0);
        let mut max_computed_height = Px(0);

        let mut children = NodeList::<Layout<'a>>::new();
        for (proportion, pane) in &self.panes {
            let max_width = *proportion * constraints.max_size.width;

            let pane_constraints = LayoutConstraint {
                min_size: Extent {
                    width: max_width,
                    height: Px(0),
                },
                max_size: Extent {
                    width: max_width,
                    height: constraints.max_size.height,
                },
            };

            let (pane_nodes, extent) = pane.layout(pane_constraints, layout_tree);

            let node = layout_tree
                .new_node(Layout {
                    rect: Rect::new(
                        Point {
                            x: moving_x,
                            y: Px(0),
                        },
                        extent,
                    ),
                    source: pane.as_ref(),
                })
                .unwrap();
            layout_tree.add_children(node, pane_nodes).unwrap();

            moving_x += extent.width;
            max_computed_height = max_computed_height.max(extent.height);
            children.push(layout_tree, node);
        }

        (
            children,
            Extent {
                width: moving_x,
                height: max_computed_height,
            },
        )
    }

    fn draw_self(&self, bounds: Rect, command_buffer: &mut Vec<DrawCommand>) {
        // no-op
    }
}
