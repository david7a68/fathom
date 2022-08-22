use std::cell::Cell;

use rand::random;

use crate::{
    color::Color,
    draw_command::DrawCommand,
    geometry::{Extent, Offset, Point, Px, Rect},
    shell::input::{Event, Input},
};

pub trait Widget {
    fn render_state(&self) -> &RenderState;

    fn render_state_mut(&mut self) -> &mut RenderState;

    fn for_each_child<'a>(&'a self, f: &mut dyn FnMut(&'a dyn Widget));

    fn accept_update(&mut self, context: &mut UpdateContext) -> PostUpdate;

    fn accept_layout(&self, context: &mut LayoutContext, constraints: BoxConstraint) -> Extent;

    fn accept_draw(&self, canvas: &mut Canvas, extent: Extent);
}

#[derive(Clone, Copy, Debug)]
#[must_use]
pub enum PostUpdate {
    NoChange,
    NeedsRedraw,
    // The widget needs to have the layout of its children recalculated. The
    // widget will make use of the current layout bounds in this calculation.
    NeedsLayout,
}

pub struct UpdateContext<'a> {
    input: &'a Input,
}

impl<'a> UpdateContext<'a> {
    pub fn new(input: &'a Input) -> Self {
        Self { input }
    }

    pub fn event(&self) -> Event {
        self.input.event()
    }

    pub fn cursor_position(&self) -> Point {
        self.input.cursor_position()
    }

    pub fn begin(&mut self, root: &mut dyn Widget) {
        self.update(root);
    }

    pub fn update(&mut self, widget: &mut dyn Widget) {
        // Invariant: the all widgets processed by an instance of
        // `UpdateContext` are part of the same tree.

        match widget.accept_update(self) {
            PostUpdate::NoChange => {
                // no-op
            }
            PostUpdate::NeedsRedraw => {
                // This is a no-op since we redraw the entire window every
                // frame anyway.
            }
            PostUpdate::NeedsLayout => {
                widget
                    .render_state_mut()
                    .status
                    .set(RenderObjectStatus::NeedsLayout);
            }
        }
    }

    /// Returns the bounds for the given widget that was calculated during the
    /// previous layout phase.
    ///
    /// Returns an empty [`Rect`] if the widget has not yet been bound to the
    /// render tree.
    pub fn bound_of(&mut self, widget: &dyn Widget) -> Rect {
        let render_object = widget.render_state();
        Rect::new(render_object.origin.get(), render_object.extent.get())
    }
}

#[derive(Default)]
pub struct LayoutContext {}

impl LayoutContext {
    pub fn begin(&mut self, root: &dyn Widget, window_extent: Extent) {
        assert!(root.render_state().offset.get() == Offset::zero());

        if root.render_state().extent.get() == window_extent {
            let mut subtrees_needing_layout = vec![];
            Self::collect_subtrees_needing_layout(root, &mut subtrees_needing_layout);

            for subtree in subtrees_needing_layout {
                let constraints = BoxConstraint::exact(subtree.render_state().extent.get());
                let _ = self.layout(subtree, constraints);

                // We don't need to position the subtree since it is explicitly
                // required to take up exaclty the same amount of space as it
                // did previously.
            }
        } else {
            // Since this is the root widget, the origin is always 0.
            let _ = self.layout(root, BoxConstraint::exact(window_extent));
        }
    }

    pub fn layout(&mut self, widget: &dyn Widget, constraints: BoxConstraint) -> Extent {
        let extent = widget.accept_layout(self, constraints);
        widget.render_state().set_layout(extent, constraints);
        extent
    }

    pub fn position_widget(&mut self, widget: &dyn Widget, offset: Offset) {
        widget.render_state().offset.set(offset);
    }

    /// Recursively collect the parents of widgets that requested layout during
    /// the update phase.
    fn collect_subtrees_needing_layout<'a>(
        widget: &'a dyn Widget,
        buffer: &mut Vec<&'a dyn Widget>,
    ) {
        // The most efficient way to do this is to walk the tree breadth-first and
        // find the nodes that have their status set to NeedsLayout. Their
        // parents can then be added to the buffer.

        // This works because computing the layout of an ancestor implicitly
        // requires layout of all its descendants. This has the added benefit of
        // allowing us to clear the flags of every descendant on the way down.

        // This should only happen if this widget is the root of the
        // hierarchy and the window was resized. Since the root widget needs
        // to be laid out again, all of its descendants will need to be
        // relaid anyway so we can return immediately.
        assert!(!widget.render_state().needs_layout());

        widget.for_each_child(&mut |child| {
            if child.render_state().needs_layout() {
                buffer.push(child);
            } else {
                Self::collect_subtrees_needing_layout(child, buffer);
            }
        });
    }
}

#[derive(Default)]
pub struct Canvas {
    current_offset: Offset,
    command_buffer: Vec<DrawCommand>,
}

impl Canvas {
    pub fn finish(self) -> Vec<DrawCommand> {
        self.command_buffer
    }

    pub fn draw(&mut self, widget: &dyn Widget) {
        let render_state = widget.render_state();
        self.current_offset += render_state.offset.get();

        // push clip bounds

        render_state.origin.set(Point::zero() + self.current_offset);
        widget.accept_draw(self, render_state.extent.get());

        // pop clip bounds

        self.current_offset -= render_state.offset.get();
    }

    /// Draws a colored rectangle at the given relative coordinates.
    pub fn fill_rect(&mut self, rect: Rect, color: Color) {
        // convert the rect into absolute coordinates
        let rect = rect + self.current_offset;

        self.command_buffer.push(DrawCommand::Rect(rect, color));
    }
}

pub struct Center<W: Widget + 'static> {
    render_state: RenderState,
    pub child: W,
}

impl<W: Widget + 'static> Center<W> {
    pub fn new(child: W) -> Self {
        Self {
            render_state: RenderState::default(),
            child,
        }
    }
}

impl<W: Widget + 'static> Widget for Center<W> {
    fn render_state(&self) -> &RenderState {
        &self.render_state
    }

    fn render_state_mut(&mut self) -> &mut RenderState {
        &mut self.render_state
    }

    fn for_each_child<'a>(&'a self, f: &mut dyn FnMut(&'a dyn Widget)) {
        f(&self.child);
    }

    fn accept_update(&mut self, context: &mut UpdateContext) -> PostUpdate {
        context.update(&mut self.child);
        PostUpdate::NoChange
    }

    fn accept_layout(&self, context: &mut LayoutContext, constraints: BoxConstraint) -> Extent {
        let child_extent = context.layout(&self.child, constraints);
        let child_offset = Offset {
            x: (constraints.max.width - child_extent.width) / 2,
            y: (constraints.max.height - child_extent.height) / 2,
        };
        context.position_widget(&self.child, child_offset);

        constraints.max
    }

    fn accept_draw(&self, canvas: &mut Canvas, _extent: Extent) {
        canvas.draw(&self.child);
    }
}

pub struct Column<W: Widget + 'static> {
    render_state: RenderState,
    children: Vec<W>,
    spacing: Px,
    needs_layout: bool,
}

impl<W: Widget + 'static> Column<W> {
    pub fn new() -> Self {
        Self {
            render_state: RenderState::default(),
            children: Vec::new(),
            spacing: Px(4),
            needs_layout: false,
        }
    }

    pub fn with_children(children: Vec<W>) -> Self {
        Self {
            render_state: RenderState::default(),
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

impl<W: Widget + 'static> Default for Column<W> {
    fn default() -> Self {
        Self::new()
    }
}

impl<W: Widget + 'static> Widget for Column<W> {
    fn render_state(&self) -> &RenderState {
        &self.render_state
    }

    fn render_state_mut(&mut self) -> &mut RenderState {
        &mut self.render_state
    }

    fn for_each_child<'a>(&'a self, f: &mut dyn FnMut(&'a dyn Widget)) {
        for child in &self.children {
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

    fn accept_layout(&self, context: &mut LayoutContext, constraints: BoxConstraint) -> Extent {
        let mut advancing_y = Px(0);
        let mut max_width = Px(0);

        // todo: padding-before

        for child in &self.children {
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
            );

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

pub struct Fill {
    render_state: RenderState,
    pub color: Color,
}

impl Fill {
    pub fn new(color: Color) -> Self {
        Self {
            render_state: RenderState::default(),
            color,
        }
    }
}

impl Widget for Fill {
    fn render_state(&self) -> &RenderState {
        &self.render_state
    }

    fn render_state_mut(&mut self) -> &mut RenderState {
        &mut self.render_state
    }

    fn for_each_child<'a>(&'a self, _: &mut dyn FnMut(&'a dyn Widget)) {}

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

    fn accept_layout(&self, _context: &mut LayoutContext, constraints: BoxConstraint) -> Extent {
        constraints.max
    }

    fn accept_draw(&self, canvas: &mut Canvas, extent: Extent) {
        canvas.fill_rect(Rect::new(Point::zero(), extent), self.color);
    }
}

pub struct SizedBox<W: Widget + 'static> {
    render_state: RenderState,
    pub extent: Extent,
    pub child: W,
}

impl<W: Widget + 'static> SizedBox<W> {
    pub fn new(extent: Extent, child: W) -> Self {
        Self {
            render_state: RenderState::default(),
            extent,
            child,
        }
    }
}

impl<W: Widget + 'static> Widget for SizedBox<W> {
    fn render_state(&self) -> &RenderState {
        &self.render_state
    }

    fn render_state_mut(&mut self) -> &mut RenderState {
        &mut self.render_state
    }

    fn for_each_child<'a>(&'a self, f: &mut dyn FnMut(&'a dyn Widget)) {
        f(&self.child);
    }

    fn accept_update(&mut self, context: &mut UpdateContext) -> PostUpdate {
        context.update(&mut self.child);
        PostUpdate::NoChange
    }

    fn accept_layout(&self, context: &mut LayoutContext, constraints: BoxConstraint) -> Extent {
        let _ = context.layout(&self.child, BoxConstraint::exact(self.extent));
        context.position_widget(&self.child, Offset::zero());
        constraints.max_fit(self.extent)
    }

    fn accept_draw(&self, canvas: &mut Canvas, _extent: Extent) {
        canvas.draw(&self.child);
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct BoxConstraint {
    min: Extent,
    max: Extent,
}

impl BoxConstraint {
    pub fn exact(extent: Extent) -> Self {
        Self {
            min: extent,
            max: extent,
        }
    }

    /// Computes the largest extent that fits within the given constraints.
    pub fn max_fit(&self, extent: Extent) -> Extent {
        Extent {
            width: self.min.width.max(extent.width.min(self.max.width)),
            height: self.min.width.max(extent.height.min(self.max.height)),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, PartialOrd)]
#[repr(u8)]
enum RenderObjectStatus {
    #[default]
    NeedsLayout,
    Ready,
}

#[derive(Debug, Default)]
pub struct RenderState {
    /// Determines if the widget needs to be laid out. This is set during the
    /// update phase and is cleared during the layout phase.
    status: Cell<RenderObjectStatus>,

    /// The position of the widget in absolute coordinates. This is set during
    /// the rendering phase.
    origin: Cell<Point>,

    /// The offset (relative position) of this widget's origin from its parent.
    /// This is set during the layout phase.
    offset: Cell<Offset>,

    /// The size of the widget's bounding box. This is set during the layout
    /// phase.
    extent: Cell<Extent>,

    /// These constraints (set during the layout phase) need to be preserved in
    /// the case that the extent exceeds them. This might happen for example if
    /// the widget's children exceed the constraints themselves. In this case,
    /// it might be up to the renderer to perform clipping operations.
    constraints: Cell<BoxConstraint>,
}

impl RenderState {
    pub fn needs_layout(&self) -> bool {
        self.status.get() == RenderObjectStatus::NeedsLayout
    }

    fn set_layout(&self, extent: Extent, constraints: BoxConstraint) {
        self.status.set(RenderObjectStatus::Ready);
        self.extent.set(extent);
        self.constraints.set(constraints);
    }
}
