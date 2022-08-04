use std::fmt::Debug;

use crate::{
    color::Color,
    draw_command::DrawCommand,
    geometry::{Extent, Point, Px, Rect},
    indexed_tree::{Index, IndexedTree, NodeList},
};

type LayoutTree<'a> = IndexedTree<Layout<'a>>;

#[must_use]
pub struct Layout<'a> {
    rect: Rect,
    source: &'a dyn RetainedElement,
}

pub struct LayoutConstraint {
    min_size: Extent,
    max_size: Extent,
}

pub trait RetainedElement: Debug {
    fn update(&self);

    fn layout<'a>(
        &'a self,
        constraints: LayoutConstraint,
        layout_tree: &mut LayoutTree<'a>,
    ) -> (NodeList<Layout<'a>>, Extent);

    fn draw_self(&self, bounds: Rect, command_buffer: &mut Vec<DrawCommand>);
}

pub struct Context {
    window_size: Extent,
    cursor_position: Option<Point>,
    background_color: Color,
    draw_commands: Vec<DrawCommand>,
    ui_root: Option<Box<dyn RetainedElement>>,
}

impl Context {
    pub fn new(window_size: Extent, background_color: Color) -> Self {
        Self {
            window_size,
            cursor_position: None,
            background_color,
            draw_commands: Vec::new(),
            ui_root: None,
        }
    }

    pub fn background_color(&self) -> Color {
        self.background_color
    }

    pub fn set_background_color(&mut self, new_color: Color) -> Color {
        let old_color = self.background_color;
        self.background_color = new_color;
        old_color
    }

    pub fn draw_commands(&self) -> &Vec<DrawCommand> {
        &self.draw_commands
    }

    pub fn update_size(&mut self, extent: Extent) {
        self.window_size = extent;
    }

    pub fn update_cursor(&mut self, position: Point) {
        self.cursor_position = Some(position);
    }

    pub fn cursor(&self) -> Option<Point> {
        self.cursor_position
    }

    pub fn set_root(&mut self, root: Box<dyn RetainedElement>) -> Option<Box<dyn RetainedElement>> {
        let mut root = Some(root);
        std::mem::swap(&mut root, &mut self.ui_root);
        root
    }

    pub fn update(&mut self) {
        let root = self.ui_root.as_mut().unwrap();
        root.update();

        // Must be before layout_tree because it it must live longer than
        // layout_tree.
        let layout_root = LayoutRoot {
            next: root.as_ref(),
        };

        let mut layout_tree = LayoutTree::new();

        let root_layout = {
            let root_constraints = LayoutConstraint {
                min_size: self.window_size,
                max_size: self.window_size,
            };

            let (nodes, extent) = root.layout(root_constraints, &mut layout_tree);

            let node = layout_tree
                .new_node(Layout {
                    rect: Rect::new(Point { x: Px(0), y: Px(0) }, extent),
                    source: &layout_root,
                })
                .unwrap();

            layout_tree.add_children(node, nodes).unwrap();

            node
        };

        self.draw_commands.clear();
        Self::collect_draw_commands(&layout_tree, root_layout, &mut self.draw_commands);
    }

    fn collect_draw_commands<'a>(
        layout_tree: &LayoutTree<'a>,
        node: Index<Layout<'a>>,
        buffer: &mut Vec<DrawCommand>,
    ) {
        let layout = layout_tree.get(node).unwrap();
        layout.source.draw_self(layout.rect, buffer);

        for child_id in layout_tree.children_ids(node) {
            Self::collect_draw_commands(layout_tree, child_id, buffer);
        }
    }
}

#[derive(Debug)]
struct LayoutRoot<'a> {
    next: &'a dyn RetainedElement,
}

impl<'a> RetainedElement for LayoutRoot<'a> {
    fn update(&self) {
        self.next.update();
    }

    fn layout<'b>(
        &'b self,
        constraints: LayoutConstraint,
        layout_tree: &mut LayoutTree<'b>,
    ) -> (NodeList<Layout<'b>>, Extent) {
        let (nodes, extent) = self.next.layout(constraints, layout_tree);

        let node = layout_tree
            .new_node(Layout {
                rect: Rect::new(Point { x: Px(0), y: Px(0) }, extent),
                source: self.next,
            })
            .unwrap();
        layout_tree.add_children(node, nodes).unwrap();

        let mut list = NodeList::new();
        list.push(layout_tree, node);

        (list, extent)
    }

    fn draw_self(&self, bounds: Rect, command_buffer: &mut Vec<DrawCommand>) {
        self.next.draw_self(bounds, command_buffer);
    }
}

#[derive(Debug)]
pub struct XSplitPanel {
    pub panes: Vec<(f32, Box<dyn RetainedElement>)>,
}

impl RetainedElement for XSplitPanel {
    fn update(&self) {
        // no-op
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

#[derive(Debug)]
pub struct ColorFill(pub Color);

impl RetainedElement for ColorFill {
    fn update(&self) {
        // no-op
    }

    fn layout<'b>(
        &'b self,
        constraints: LayoutConstraint,
        _layout_tree: &mut LayoutTree<'b>,
    ) -> (NodeList<Layout<'b>>, Extent) {
        (NodeList::new(), constraints.max_size)
    }

    fn draw_self(&self, bounds: Rect, command_buffer: &mut Vec<DrawCommand>) {
        command_buffer.push(DrawCommand::Rect(bounds, self.0));
    }
}
