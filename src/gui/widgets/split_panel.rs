use crate::{
    gfx::geometry::{Extent, Offset, Px},
    gui::input::Event,
};

use super::{
    BoxConstraint, DrawContext, LayoutContext, PostUpdate, UpdateContext, Widget, WidgetState,
};

pub enum Axis {
    X,
    Y,
}

#[must_use]
pub struct SplitPanel<W: Widget + 'static> {
    state: WidgetState,
    children: Vec<W>,
    axis: Axis,
}

impl<W: Widget + 'static> SplitPanel<W> {
    pub fn with_children(axis: Axis, children: Vec<W>) -> Self {
        Self {
            state: WidgetState::default(),
            children,
            axis,
        }
    }
}

impl<W: Widget + 'static> Widget for SplitPanel<W> {
    fn widget_state(&self) -> &WidgetState {
        &self.state
    }

    fn widget_state_mut(&mut self) -> &mut WidgetState {
        &mut self.state
    }

    fn for_each_child_mut<'a>(&'a mut self, f: &mut dyn FnMut(&'a mut dyn Widget)) {
        for child in &mut self.children {
            f(child);
        }
    }

    fn accept_update(&mut self, context: &mut UpdateContext) -> PostUpdate {
        match context.event() {
            Event::None => {}
            Event::CursorMove { position } => {
                for child in &mut self.children {
                    if context.bound_of(child).contains(position) {
                        context.update(child);
                        break;
                    }
                }
            }
            Event::MouseButton { .. } => {
                // TODO(straivers): handle keyboard focus
                for child in &mut self.children {
                    if context.bound_of(child).contains(context.cursor_position()) {
                        context.update(child);
                        break;
                    }
                }
            }
        }

        PostUpdate::NoChange
    }

    fn accept_layout(&mut self, context: &mut LayoutContext, constraints: BoxConstraint) -> Extent {
        match self.axis {
            Axis::X => {
                let per_child_width =
                    constraints.max.width / self.children.len().try_into().unwrap();
                let mut slack = constraints.max.width % self.children.len().try_into().unwrap();
                let mut advancing_x = Px(0);

                for child in &mut self.children {
                    let child_constraint = BoxConstraint::exact(Extent {
                        width: if slack > 0 {
                            slack -= 1.into();
                            per_child_width + 1.into()
                        } else {
                            per_child_width
                        },
                        height: constraints.max.height,
                    });

                    let child_extent = context.layout(child, child_constraint);
                    context.position_widget(
                        child,
                        Offset {
                            x: advancing_x,
                            y: Px(0),
                        },
                        child_extent,
                    );
                    advancing_x += child_extent.width;
                }

                constraints.max
            }
            Axis::Y => {
                let per_child_height =
                    constraints.max.height / self.children.len().try_into().unwrap();
                let mut slack = constraints.max.height % self.children.len().try_into().unwrap();
                let mut advancing_y = Px(0);

                for child in &mut self.children {
                    let child_constraint = BoxConstraint::exact(Extent {
                        width: constraints.max.width,
                        height: if slack > 0 {
                            slack -= 1.into();
                            per_child_height + 1.into()
                        } else {
                            per_child_height
                        },
                    });

                    let child_extent = context.layout(child, child_constraint);
                    context.position_widget(
                        child,
                        Offset {
                            x: Px(0),
                            y: advancing_y,
                        },
                        child_extent,
                    );
                    advancing_y += child_extent.height;
                }

                constraints.max
            }
        }
    }

    fn accept_draw(&self, canvas: &mut DrawContext, _extent: Extent) {
        for child in &self.children {
            canvas.draw(child);
        }
    }
}
