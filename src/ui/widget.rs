use crate::{indexed_tree::NodeList, geometry::{Extent, Rect}, draw_command::DrawCommand};

use super::{Input, LayoutConstraint, LayoutTree, Layout};

pub trait Widget {
    fn update(&self, input: &Input);

    fn layout<'a>(
        &'a self,
        constraints: LayoutConstraint,
        layout_tree: &mut LayoutTree<'a>,
    ) -> (NodeList<Layout<'a>>, Extent);

    fn draw_self(&self, bounds: Rect, command_buffer: &mut Vec<DrawCommand>);
}
