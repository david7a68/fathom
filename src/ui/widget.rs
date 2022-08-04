use crate::{
    draw_command::DrawCommand,
    geometry::{Extent, Rect},
    indexed_tree::NodeList,
};

use super::{Input, Layout, LayoutConstraint, LayoutTree};

pub trait Widget {
    fn update(&self, input: &Input);

    fn layout<'a>(
        &'a self,
        constraints: LayoutConstraint,
        layout_tree: &mut LayoutTree<'a>,
    ) -> (NodeList<Layout<'a>>, Extent);

    fn draw_self(&self, bounds: Rect, command_buffer: &mut Vec<DrawCommand>);
}
