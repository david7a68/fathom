pub mod debug;
pub mod layout;
pub mod split_panel;
pub mod tabbed_panel;

use crate::{
    gfx::{
        canvas::{Canvas, Paint},
        geometry::{Extent, Offset, Point, Rect},
    },
    shell::input::{Event, Input},
};

pub trait Widget {
    fn widget_state(&self) -> &WidgetState;

    fn widget_state_mut(&mut self) -> &mut WidgetState;

    fn for_each_child_mut<'a>(&'a mut self, f: &mut dyn FnMut(&'a mut dyn Widget));

    fn accept_update(&mut self, context: &mut UpdateContext) -> PostUpdate;

    fn accept_layout(&mut self, context: &mut LayoutContext, constraints: BoxConstraint) -> Extent;

    fn accept_draw(&self, canvas: &mut DrawContext, extent: Extent);
}

/// Implementing [`Widget`] for `Box<dyn Widget>` permits a few nifty
/// capabilities that are highly desirable; namely the ability to shorten static
/// type names (and thus improve compile times of complex widget trees), and the
/// ability to store multiple kinds of widgets within the same layout.
///
/// Naturally, this invokes a performance penalty for dynamic dispatch, but
/// offers increased flexibility for minimal cost when used judiciously.
/// Furthermore, there is no cost for implementing [`Widget`] for Box beyond the
/// Box itself, since the static dispatch can be easily inlined away.
impl Widget for Box<dyn Widget> {
    #[inline]
    fn widget_state(&self) -> &WidgetState {
        self.as_ref().widget_state()
    }

    #[inline]
    fn widget_state_mut(&mut self) -> &mut WidgetState {
        self.as_mut().widget_state_mut()
    }

    #[inline]
    fn for_each_child_mut<'a>(&'a mut self, f: &mut dyn FnMut(&'a mut dyn Widget)) {
        self.as_mut().for_each_child_mut(f)
    }

    #[inline]
    fn accept_update(&mut self, context: &mut UpdateContext) -> PostUpdate {
        self.as_mut().accept_update(context)
    }

    #[inline]
    fn accept_layout(&mut self, context: &mut LayoutContext, constraints: BoxConstraint) -> Extent {
        self.as_mut().accept_layout(context, constraints)
    }

    #[inline]
    fn accept_draw(&self, canvas: &mut DrawContext, extent: Extent) {
        self.as_ref().accept_draw(canvas, extent);
    }
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
                widget.widget_state_mut().set_needs_layout();
            }
        }
    }

    /// Returns the bounds for the given widget that was calculated during the
    /// previous layout phase.
    ///
    /// Returns an empty [`Rect`] if the widget has not yet been bound to the
    /// render tree.
    pub fn bound_of(&mut self, widget: &dyn Widget) -> Rect {
        widget.widget_state().rect()
    }
}

#[derive(Default)]
pub struct LayoutContext {}

impl LayoutContext {
    pub fn begin(&mut self, root: &mut dyn Widget, window_extent: Extent) {
        assert!(root.widget_state().offset() == Offset::zero());

        if root.widget_state().extent() == window_extent {
            let mut subtrees_needing_layout = vec![];
            Self::collect_subtrees_needing_layout(root, &mut subtrees_needing_layout);

            for subtree in subtrees_needing_layout {
                let constraints = BoxConstraint::exact(subtree.widget_state().extent());
                let _ = self.layout(subtree, constraints);

                // Now that we have the subtree's layout, we can update the
                // origins of its children (and they're more likely to be in
                // cache here).
                Self::update_origins(subtree);
            }
        } else {
            // Since this is the root widget, the origin is always 0.
            let _ = self.layout(root, BoxConstraint::exact(window_extent));
            root.widget_state_mut()
                .set_layout(Offset::zero(), window_extent);
            Self::update_origins(root);
        }
    }

    pub fn layout(&mut self, widget: &mut dyn Widget, constraints: BoxConstraint) -> Extent {
        widget.accept_layout(self, constraints)
    }

    pub fn position_widget(&mut self, widget: &mut dyn Widget, offset: Offset, extent: Extent) {
        widget.widget_state_mut().set_layout(offset, extent);
    }

    /// Recursively collect the parents of widgets that requested layout during
    /// the update phase.
    fn collect_subtrees_needing_layout<'a>(
        widget: &'a mut dyn Widget,
        buffer: &mut Vec<&'a mut dyn Widget>,
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
        assert!(!widget.widget_state().needs_layout());

        widget.for_each_child_mut(&mut |child| {
            if child.widget_state().needs_layout() {
                buffer.push(child);
            } else {
                Self::collect_subtrees_needing_layout(child, buffer);
            }
        });
    }

    fn update_origins(widget: &mut dyn Widget) {
        let origin = widget.widget_state().origin();
        widget.for_each_child_mut(&mut |child| {
            let child_offset = child.widget_state().offset();
            child.widget_state_mut().set_origin(origin + child_offset);
            Self::update_origins(child);
        });
    }
}

pub struct DrawContext<'a> {
    canvas: &'a mut dyn Canvas,
    current_offset: Offset,
}

impl<'a> DrawContext<'a> {
    pub fn new(canvas: &'a mut dyn Canvas) -> Self {
        Self {
            canvas,
            current_offset: Offset::zero(),
        }
    }

    pub fn draw(&mut self, widget: &dyn Widget) {
        let widget_state = widget.widget_state();
        self.current_offset += widget_state.offset();

        // push clip bounds

        widget.accept_draw(self, widget_state.extent());

        // pop clip bounds

        self.current_offset -= widget_state.offset();
    }

    /// Draws a colored rectangle at the given relative coordinates.
    pub fn draw_rect(&mut self, rect: Rect, paint: &Paint) {
        // convert the rect into absolute coordinates
        let rect = rect + self.current_offset;
        self.canvas.draw_rect(rect, paint);
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

#[derive(Clone, Copy, Default)]
struct Layout {
    /// The offset (relative position) of this widget's origin from its parent.
    offset: Offset,
    /// The size of the widget's bounding box.
    extent: Extent,
}

#[derive(Default)]
pub struct WidgetState {
    /// Determines if the widget needs to be laid out. This is set during the
    /// update phase and is cleared during the layout phase.
    status: RenderObjectStatus,

    /// The position of the widget in absolute coordinates. This is set during
    /// the rendering phase.
    origin: Point,

    layout: Layout,
}

impl WidgetState {
    fn set_needs_layout(&mut self) {
        self.status = RenderObjectStatus::NeedsLayout;
    }

    fn needs_layout(&self) -> bool {
        self.status == RenderObjectStatus::NeedsLayout
    }

    fn offset(&self) -> Offset {
        self.layout.offset
    }

    fn origin(&self) -> Point {
        self.origin
    }

    fn extent(&self) -> Extent {
        self.layout.extent
    }

    fn rect(&self) -> Rect {
        Rect::new(self.origin(), self.extent())
    }

    fn set_origin(&mut self, origin: Point) {
        self.origin = origin;
    }

    fn set_layout(&mut self, offset: Offset, extent: Extent) {
        self.status = RenderObjectStatus::Ready;
        self.layout = Layout { offset, extent };
    }
}
