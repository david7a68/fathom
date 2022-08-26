//! This module defines a multi-panel widget with tabs (think editor tabs in
//! VSCode).

use std::num::NonZeroU8;

use crate::{
    geometry::{Extent, Point, Rect},
    shell::input::Event,
};

use super::{BoxConstraint, Canvas, LayoutContext, PostUpdate, UpdateContext, Widget, WidgetState};

/// A hierarchy of split panels; the leaves of which contain tabs that may be
/// dragged between panels.
///
/// Todo:
///
///  - [ ] Implement Widget (basic)
///    - [ ] Draw plain background
///    - [ ] Draw hardcoded panels
///    - [ ] Identify when a cursor is hovering over the edge between two panels
///    - [ ] Draw a colored rect over the hover area
///    - [ ] Handle dragging the hover area
pub struct TabbedPanel<W: Widget> {
    state: WidgetState,
    node: Vec<Node>,
    widgets: Vec<W>,
}

impl<W: Widget> TabbedPanel<W> {
    pub fn new(default: W) -> Self {
        Self {
            state: WidgetState::default(),
            node: vec![Node {
                prev: None,
                next: None,
                bounds: Rect::zero(),
                state: State::Leaf { widget: 0 },
            }],
            widgets: vec![default],
        }
    }

    fn smallest_pane_containing(&self, point: Point) -> u8 {
        fn recurse(nodes: &[Node], node: &Node, idx: usize, point: Point) -> usize {
            match node.state {
                State::Leaf { .. } => idx,
                State::FixedInner { first, .. } | State::AutoInner { first, .. } => {
                    let mut child_index = Some(first);
                    while let Some(child_idx) = child_index.map(|i| i.get() as usize) {
                        let child = &nodes[child_idx];
                        if child.bounds.contains(point) {
                            return recurse(nodes, child, child_idx, point);
                        } else {
                            child_index = child.next;
                        }
                    }

                    panic!(
                        "inner pane ({}) contains point {:?} but none of its children do",
                        idx, point
                    );
                }
            }
        }

        recurse(&self.node, &self.node[0], 0, point)
            .try_into()
            .unwrap()
    }
}

impl<W: Widget> Widget for TabbedPanel<W> {
    fn widget_state(&self) -> &WidgetState {
        &self.state
    }

    fn widget_state_mut(&mut self) -> &mut WidgetState {
        &mut self.state
    }

    fn for_each_child_mut<'a>(&'a mut self, f: &mut dyn FnMut(&'a mut dyn Widget)) {
        for widget in &mut self.widgets {
            f(widget)
        }
    }

    fn accept_update(&mut self, context: &mut UpdateContext) -> PostUpdate {
        match context.event() {
            Event::None => {}
            Event::CursorMove { .. } | Event::MouseButton { .. } => {
                let point = context.cursor_position();
                if context.bound_of(self).contains(point) {
                    let pane = &self.node[self.smallest_pane_containing(point) as usize];
                    // todo: set focus on MouseButton
                    if let State::Leaf { widget } = pane.state {
                        context.update(&mut self.widgets[widget as usize])
                    } else {
                        unreachable!()
                    }
                }
            }
        }

        PostUpdate::NoChange
    }

    fn accept_layout(&mut self, context: &mut LayoutContext, constraints: BoxConstraint) -> Extent {
        // todo: compute child layouts

        constraints.max
    }

    fn accept_draw(&self, canvas: &mut Canvas, extent: Extent) {
        // for each widget, draw
    }
}

#[repr(u8)]
#[derive(Debug, Default, PartialEq, Eq)]
enum Mode {
    #[default]
    Undefined,
    AutoX,
    AutoY,
    FixedX,
    FixedY,
}

struct Node {
    prev: Option<NonZeroU8>,
    next: Option<NonZeroU8>,
    bounds: Rect,
    state: State,
}

enum State {
    Leaf {
        widget: u8,
    },
    FixedInner {
        first: NonZeroU8,
        last: NonZeroU8,
        layout_mode: Mode,
        percent_of_parent: f32,
    },
    AutoInner {
        first: NonZeroU8,
        last: NonZeroU8,
        layout_mode: Mode,
    },
}
