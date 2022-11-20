use rand::random;

use crate::{
    gfx::{
        color::Color,
        geometry::{Extent, Point, Rect},
        Image, Paint,
    },
    gui::input::Event,
    handle_pool::Handle,
};

use super::{
    BoxConstraint, DrawContext, LayoutContext, PostUpdate, UpdateContext, Widget, WidgetState,
};

#[must_use]
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
            Event::None | Event::CursorMove { .. } => PostUpdate::NoChange,
            Event::MouseButton { button, state } => {
                if button.is_left() && state.is_pressed() {
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
        canvas.draw_rect(
            Rect::new(Point::zero(), extent),
            &Paint::Fill { color: self.color },
        );
    }
}

#[must_use]
pub struct FillImage {
    widget_state: WidgetState,
    image: Handle<Image>,
    image_extent: Extent,
}

impl FillImage {
    pub fn new(image: Handle<Image>, image_extent: Extent) -> Self {
        Self {
            widget_state: WidgetState::default(),
            image,
            image_extent,
        }
    }
}

impl Widget for FillImage {
    fn widget_state(&self) -> &WidgetState {
        &self.widget_state
    }

    fn widget_state_mut(&mut self) -> &mut WidgetState {
        &mut self.widget_state
    }

    fn for_each_child_mut<'a>(&'a mut self, _: &mut dyn FnMut(&'a mut dyn Widget)) {}

    fn accept_update(&mut self, context: &mut UpdateContext) -> PostUpdate {
        PostUpdate::NoChange
    }

    fn accept_layout(
        &mut self,
        _context: &mut LayoutContext,
        constraints: BoxConstraint,
    ) -> Extent {
        constraints.max
    }

    fn accept_draw(&self, canvas: &mut DrawContext, extent: Extent) {
        canvas.draw_image(
            Rect::new(Point::zero(), extent),
            self.image,
            Rect::new(Point::zero(), self.image_extent),
            &Paint::Fill {
                color: Color::WHITE,
            },
        );
    }
}
