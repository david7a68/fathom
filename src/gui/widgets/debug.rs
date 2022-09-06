use rand::random;

use crate::{
    gfx::{
        color::Color,
        geometry::{Extent, Point, Rect},
    },
    shell::input::Event,
};

use super::{BoxConstraint, DrawContext, LayoutContext, PostUpdate, UpdateContext, Widget, WidgetState};

pub struct Fill {
    widget_state: WidgetState,
    pub color: Color,
}

impl Fill {
    pub fn new(color: Color) -> Self {
        Self {
            widget_state: WidgetState::default(),
            color,
        }
    }
}

impl Widget for Fill {
    fn widget_state(&self) -> &WidgetState {
        &self.widget_state
    }

    fn widget_state_mut(&mut self) -> &mut WidgetState {
        &mut self.widget_state
    }

    fn for_each_child_mut<'a>(&'a mut self, _: &mut dyn FnMut(&'a mut dyn Widget)) {}

    fn accept_update(&mut self, context: &mut UpdateContext) -> PostUpdate {
        match context.event() {
            Event::None => PostUpdate::NoChange,
            Event::CursorMove { .. } => PostUpdate::NoChange,
            Event::MouseButton { button, state } => {
                if button.is_left() && state.is_released() {
                    self.color = random();
                    PostUpdate::NeedsRedraw
                } else {
                    PostUpdate::NoChange
                }
            }
        }
    }

    fn accept_layout(
        &mut self,
        _context: &mut LayoutContext,
        constraints: BoxConstraint,
    ) -> Extent {
        constraints.max
    }

    fn accept_draw(&self, canvas: &mut DrawContext, extent: Extent) {
        canvas.fill_rect(Rect::new(Point::zero(), extent), self.color);
    }
}
