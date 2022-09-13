use rand::random;

use crate::{
    gfx::{
        canvas::Paint,
        color::Color,
        geometry::{Extent, Offset, Px, Rect},
    },
    shell::input::{ButtonState, Event, MouseButton},
};

use super::{
    BoxConstraint, DrawContext, LayoutContext, PostUpdate, UpdateContext, Widget, WidgetState,
};

const TAB_BAR_HEIGHT: Px = Px(10);
const TAB_WIDTH: Px = Px(30);

pub struct TabbedPanel<W: Widget> {
    state: WidgetState,
    children: Vec<Tab<W>>,
    active: usize,
}

impl<W: Widget> TabbedPanel<W> {
    pub fn with_children(mut children: Vec<W>) -> Self {
        Self {
            state: WidgetState::default(),
            children: children
                .drain(..)
                .map(|c| Tab {
                    width: TAB_WIDTH,
                    widget: c,
                    color: random(),
                })
                .collect(),
            active: 0,
        }
    }

    fn tab_bar_rect(&self, bounds: Rect) -> Rect {
        Rect {
            left: bounds.left,
            right: bounds.right,
            top: bounds.top,
            bottom: bounds.top + TAB_BAR_HEIGHT,
        }
    }

    fn content_rect(&self, bounds: Rect) -> Rect {
        Rect {
            left: bounds.left,
            right: bounds.right,
            top: bounds.top + TAB_BAR_HEIGHT,
            bottom: bounds.bottom,
        }
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
        for child in &mut self.children {
            f(&mut child.widget)
        }
    }

    fn accept_update(&mut self, context: &mut UpdateContext) -> PostUpdate {
        let rect = context.bound_of(self);

        match context.event() {
            Event::None => PostUpdate::NoChange,
            Event::CursorMove { position } => {
                if self.content_rect(rect).contains(position) {
                    context.update(&mut self.children[self.active].widget);
                }

                PostUpdate::NoChange
            }
            Event::MouseButton { button, state } => {
                let cursor_pos = context.cursor_position();

                if self.tab_bar_rect(rect).contains(cursor_pos) {
                    let cursor_x = cursor_pos.x;
                    let mut advancing_x = rect.left;
                    for (i, child) in self.children.iter_mut().enumerate() {
                        advancing_x += child.width;
                        if cursor_x <= advancing_x {
                            if button == MouseButton::Left && state == ButtonState::Pressed {
                                self.active = i;
                                return PostUpdate::NeedsLayout;
                            }

                            break;
                        }
                    }

                    PostUpdate::NoChange
                } else if self.content_rect(rect).contains(cursor_pos) {
                    context.update(&mut self.children[self.active].widget);
                    PostUpdate::NoChange
                } else {
                    PostUpdate::NoChange
                }
            }
        }
    }

    fn accept_layout(&mut self, context: &mut LayoutContext, constraints: BoxConstraint) -> Extent {
        let child_constraints = BoxConstraint::exact(Extent {
            width: constraints.max.width,
            height: constraints.max.height - TAB_BAR_HEIGHT,
        });

        let child_extent =
            context.layout(&mut self.children[self.active].widget, child_constraints);

        context.position_widget(
            &mut self.children[self.active].widget,
            Offset {
                x: Px(0),
                y: TAB_BAR_HEIGHT,
            },
            child_extent,
        );

        constraints.max
    }

    fn accept_draw(&self, canvas: &mut DrawContext, _extent: Extent) {
        let mut advancing_x = Px(0);
        for child in &self.children {
            canvas.draw_rect(
                Rect {
                    left: advancing_x,
                    right: advancing_x + child.width,
                    top: Px(0),
                    bottom: TAB_BAR_HEIGHT,
                },
                &Paint::Fill { color: child.color },
            );

            advancing_x += child.width;
        }

        canvas.draw(&self.children[self.active].widget);
    }
}

struct Tab<W: Widget> {
    width: Px,
    color: Color,
    widget: W,
}
