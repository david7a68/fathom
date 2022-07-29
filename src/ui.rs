use std::cell::Cell;

use rand::Rng;

use crate::{
    color::Color,
    indexed_store::{Index, IndexedStore},
    point::Point,
    renderer::Vertex,
    shapes::Rect,
};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PanelId(Index);

impl From<Index> for PanelId {
    fn from(i: Index) -> Self {
        Self(i)
    }
}

pub struct Context {
    width: u32,
    height: u32,
    cursor_position: Option<Point>,
    background_color: Color,
    vertex_buffer: Vec<Vertex>,
    index_buffer: Vec<u16>,
    allocator: IndexedStore<Panel>,
    root_panel: PanelId,
}

impl Context {
    pub fn new(width: u32, height: u32, background_color: Color) -> Self {
        let mut allocator = IndexedStore::new();

        let root_panel = allocator
            .insert(Panel {
                portion_of_parent: 1.0,
                color: background_color,
                next: PanelId::default(),
                prev: PanelId::default(),
                first_child: PanelId::default(),
                cached_bounds: Cell::new(Rect {
                    top: 0.0,
                    left: 0.0,
                    bottom: height as f32,
                    right: width as f32,
                }),
            })
            .unwrap();

        Self {
            width,
            height,
            cursor_position: None,
            background_color,
            vertex_buffer: Vec::new(),
            index_buffer: Vec::new(),
            allocator,
            root_panel: PanelId(root_panel),
        }
    }

    pub fn background_color(&self) -> Color {
        self.background_color
    }

    pub fn set_background_color(&mut self, new_color: Color) -> Color {
        let old_color = self.background_color;
        self.background_color = new_color;
        old_color
    }

    pub fn vertex_buffer(&self) -> &Vec<Vertex> {
        &self.vertex_buffer
    }

    pub fn index_buffer(&self) -> &Vec<u16> {
        &self.index_buffer
    }

    pub fn update_size(&mut self, width: u32, height: u32) {
        self.width = width;
        self.height = height;
    }

    pub fn update_cursor(&mut self, position: Point) {
        self.cursor_position = Some(position);
    }

    pub fn cursor(&self) -> Option<Point> {
        self.cursor_position
    }

    pub fn root_panel(&self) -> PanelId {
        self.root_panel
    }

    pub fn split_panel(&mut self, parent_idx: PanelId, proportion: f32) -> (PanelId, PanelId) {
        let parent = self.allocator.get(parent_idx.0).unwrap();
        assert!(parent.first_child == PanelId::default());

        let parent_rect = parent.cached_bounds.get();
        let mut rng = rand::thread_rng();

        let first = self
            .allocator
            .insert(Panel {
                portion_of_parent: proportion,
                color: rng.gen(),
                next: PanelId::default(),
                prev: PanelId::default(),
                first_child: PanelId::default(),
                cached_bounds: Cell::new(Rect {
                    top: parent_rect.top,
                    left: parent_rect.left,
                    bottom: parent_rect.bottom,
                    right: parent_rect.right * proportion,
                }),
            })
            .unwrap()
            .into();

        let second = self
            .allocator
            .insert(Panel {
                portion_of_parent: 1.0 - proportion,
                color: rng.gen(),
                next: PanelId::default(),
                prev: first,
                first_child: PanelId::default(),
                cached_bounds: Cell::new(Rect {
                    top: parent_rect.top,
                    left: parent_rect.right * proportion,
                    bottom: parent_rect.bottom,
                    right: parent_rect.right,
                }),
            })
            .unwrap()
            .into();

        {
            let first = self.allocator.get_mut(first.0).unwrap();
            first.next = second;
        }

        let parent = self.allocator.get_mut(parent_idx.0).unwrap();
        assert!(parent.first_child == PanelId::default());
        parent.first_child = first;

        (first, second)
    }

    pub fn panel_containing(&self, point: Point) -> PanelId {
        smallest_panel_containing(&self.allocator, self.root_panel, point)
    }

    pub fn panel_mut(&mut self, id: PanelId) -> &mut Panel {
        self.allocator.get_mut(id.0).unwrap()
    }

    pub fn update(&mut self) {
        self.vertex_buffer.clear();
        self.index_buffer.clear();

        update_panels(
            &self.allocator,
            &mut self.vertex_buffer,
            &mut self.index_buffer,
            self.root_panel,
            Rect {
                top: 0.0,
                left: 0.0,
                bottom: self.height as f32,
                right: self.width as f32,
            },
        );
    }
}

trait Node {
    fn layout(&self, context: &mut Context, parent_rect: Rect);

    fn draw(&self, context: &mut Context);
}

#[derive(Debug)]
pub struct Panel {
    portion_of_parent: f32,
    pub color: Color,

    next: PanelId,
    prev: PanelId,

    first_child: PanelId,
    cached_bounds: Cell<Rect>,
}

fn smallest_panel_containing(
    panels: &IndexedStore<Panel>,
    panel_idx: PanelId,
    point: Point,
) -> PanelId {
    let panel = panels.get(panel_idx.0).unwrap();
    assert!(panel.cached_bounds.get().contains(point));

    let mut current = panel.first_child;
    while current != PanelId::default() {
        let panel = panels.get(current.0).unwrap();
        if panel.cached_bounds.get().contains(point) {
            return smallest_panel_containing(panels, current, point);
        } else {
            current = panel.next;
        }
    }

    panel_idx
}

fn update_panels(
    panels: &IndexedStore<Panel>,
    vertex_buffer: &mut Vec<Vertex>,
    index_buffer: &mut Vec<u16>,
    panel_idx: PanelId,
    parent_rect: Rect,
) {
    let panel = panels.get(panel_idx.0).unwrap();
    panel.cached_bounds.set(parent_rect);

    let width = parent_rect.right - parent_rect.left;
    let mut offset = parent_rect.left;

    parent_rect.draw(panel.color, vertex_buffer, index_buffer);

    let mut child_idx = panel.first_child;

    while child_idx != PanelId::default() {
        let child = panels.get(child_idx.0).unwrap();

        let mut child_rect = child.cached_bounds.get();
        child_rect.left = offset;
        child_rect.right = offset + width * child.portion_of_parent;
        child_rect.top = parent_rect.top;
        child_rect.bottom = parent_rect.bottom;
        offset = child_rect.right;
        child.cached_bounds.set(child_rect);

        update_panels(panels, vertex_buffer, index_buffer, child_idx, child_rect);

        child_idx = child.next;
    }
}
