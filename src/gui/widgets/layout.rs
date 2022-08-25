use crate::{
    geometry::{Extent, Offset, Px},
    shell::input::Event,
};

use super::{BoxConstraint, Canvas, LayoutContext, PostUpdate, UpdateContext, Widget, WidgetState};

pub struct Center<W: Widget + 'static> {
    widget_state: WidgetState,
    pub child: W,
}

impl<W: Widget + 'static> Center<W> {
    pub fn new(child: W) -> Self {
        Self {
            widget_state: WidgetState::default(),
            child,
        }
    }

    pub fn boxed(self) -> Box<dyn Widget> {
        Box::new(self)
    }
}

impl<W: Widget + 'static> Widget for Center<W> {
    fn widget_state(&self) -> &WidgetState {
        &self.widget_state
    }

    fn widget_state_mut(&mut self) -> &mut WidgetState {
        &mut self.widget_state
    }

    fn for_each_child_mut<'a>(&'a mut self, f: &mut dyn FnMut(&'a mut dyn Widget)) {
        f(&mut self.child);
    }

    fn accept_update(&mut self, context: &mut UpdateContext) -> PostUpdate {
        context.update(&mut self.child);
        PostUpdate::NoChange
    }

    fn accept_layout(&mut self, context: &mut LayoutContext, constraints: BoxConstraint) -> Extent {
        let child_extent = context.layout(&mut self.child, constraints);
        let child_offset = Offset {
            x: (constraints.max.width - child_extent.width) / 2,
            y: (constraints.max.height - child_extent.height) / 2,
        };
        context.position_widget(&mut self.child, child_offset, child_extent);

        constraints.max
    }

    fn accept_draw(&self, canvas: &mut Canvas, _extent: Extent) {
        canvas.draw(&self.child);
    }
}

pub struct Column<W: Widget> {
    widget_state: WidgetState,
    children: Vec<W>,
    spacing: Px,
    needs_layout: bool,
}

impl<W: Widget> Column<W> {
    pub fn new() -> Self {
        Self {
            widget_state: WidgetState::default(),
            children: Vec::new(),
            spacing: Px(4),
            needs_layout: false,
        }
    }

    pub fn with_children(children: Vec<W>) -> Self {
        Self {
            widget_state: WidgetState::default(),
            children,
            spacing: Px(4),
            needs_layout: false,
        }
    }

    pub fn with_child(mut self, child: W) -> Self {
        self.children.push(child);
        self
    }

    pub fn add(&mut self, child: W) {
        self.children.push(child);
        self.needs_layout = true;
    }

    pub fn remove(&mut self, index: usize) {
        self.children.remove(index);
        self.needs_layout = true;
    }
}

impl<W: Widget> Default for Column<W> {
    fn default() -> Self {
        Self::new()
    }
}

impl<W: Widget> Widget for Column<W> {
    fn widget_state(&self) -> &WidgetState {
        &self.widget_state
    }

    fn widget_state_mut(&mut self) -> &mut WidgetState {
        &mut self.widget_state
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
                    // If the child handles the event, there's no need to pass
                    // it to the next child.
                    if position.within(&context.bound_of(child)) {
                        context.update(child);
                    }
                }
            }
            Event::MouseButton { .. } => {
                for child in &mut self.children {
                    // If the child handles the event, there's no need to pass
                    // it to the next child.
                    if context.cursor_position().within(&context.bound_of(child)) {
                        context.update(child);
                    }
                }
            }
        }

        if self.needs_layout {
            self.needs_layout = false;
            PostUpdate::NeedsLayout
        } else {
            PostUpdate::NoChange
        }
    }

    fn accept_layout(&mut self, context: &mut LayoutContext, constraints: BoxConstraint) -> Extent {
        let mut advancing_y = Px(0);
        let mut max_width = Px(0);

        // todo: padding-before

        for child in &mut self.children {
            // reduce the available height
            let child_constraints = BoxConstraint {
                min: Extent::zero(),
                max: Extent {
                    width: constraints.max.width,
                    height: constraints.max.height - advancing_y,
                },
            };

            let child_extent = context.layout(child, child_constraints);
            context.position_widget(
                child,
                Offset {
                    x: Px(0),
                    y: advancing_y,
                },
                child_extent,
            );

            println!("advancing_y: {:?}", advancing_y);
            println!("child extent: {:?}", child_extent);

            // advance to the next widget's position
            advancing_y += child_extent.height + self.spacing;
            max_width = max_width.max(child_extent.width);
        }

        // todo: padding-after

        if advancing_y > 0 {
            // Account for the spacing between widgets taht we added above
            advancing_y -= self.spacing;
        }

        Extent {
            width: max_width,
            height: advancing_y,
        }
    }

    fn accept_draw(&self, canvas: &mut Canvas, _extent: Extent) {
        for child in &self.children {
            canvas.draw(child);
        }
    }
}

pub struct SizedBox<W: Widget> {
    widget_state: WidgetState,
    pub extent: Extent,
    pub child: W,
}

impl<W: Widget> SizedBox<W> {
    pub fn new(extent: Extent, child: W) -> Self {
        Self {
            widget_state: WidgetState::default(),
            extent,
            child,
        }
    }

    pub fn boxed(self) -> Box<Self> {
        Box::new(self)
    }
}

impl<W: Widget> Widget for SizedBox<W> {
    fn widget_state(&self) -> &WidgetState {
        &self.widget_state
    }

    fn widget_state_mut(&mut self) -> &mut WidgetState {
        &mut self.widget_state
    }

    fn for_each_child_mut<'a>(&'a mut self, f: &mut dyn FnMut(&'a mut dyn Widget)) {
        f(&mut self.child);
    }

    fn accept_update(&mut self, context: &mut UpdateContext) -> PostUpdate {
        context.update(&mut self.child);
        PostUpdate::NoChange
    }

    fn accept_layout(
        &mut self,
        context: &mut LayoutContext,
        _constraints: BoxConstraint,
    ) -> Extent {
        let _ = context.layout(&mut self.child, BoxConstraint::exact(self.extent));
        context.position_widget(&mut self.child, Offset::zero(), self.extent);
        self.extent
    }

    fn accept_draw(&self, canvas: &mut Canvas, _extent: Extent) {
        canvas.draw(&self.child);
    }
}
