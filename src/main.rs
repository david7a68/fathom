mod color;
mod event_loop;
mod indexed_store;
mod point;
mod renderer;

use std::{cell::RefCell, rc::Rc};

use color::Color;
use event_loop::{ButtonState, EventLoopControl, MouseButton, WindowHandle, WindowEventHandler};
use point::Point;

use renderer::{Renderer, SwapchainHandle, Vertex};

const TRIANGLE: [Vertex; 3] = [
    Vertex {
        point: Point { x: 0.0, y: -100.0 },
        color: Color {
            r: 1.0,
            g: 0.0,
            b: 0.0,
            a: 1.0,
        },
    },
    Vertex {
        point: Point { x: 100.0, y: 100.0 },
        color: Color {
            r: 0.0,
            g: 1.0,
            b: 0.0,
            a: 1.0,
        },
    },
    Vertex {
        point: Point {
            x: -100.0,
            y: 100.0,
        },
        color: Color {
            r: 0.0,
            g: 0.0,
            b: 1.0,
            a: 1.0,
        },
    },
];

const INDICES: [u16; 6] = [0, 1, 2, 2, 3, 0];

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let renderer = Rc::new(RefCell::new(Renderer::new()?));

    let mut event_loop = event_loop::EventLoop::new();
    event_loop.create_window(Box::new(Window::new(renderer)));
    event_loop.run();

    Ok(())
}

struct Window {
    swapchain: SwapchainHandle,
    renderer: Rc<RefCell<Renderer>>,
}

impl Window {
    fn new(renderer: Rc<RefCell<Renderer>>) -> Self {
        Self {
            swapchain: SwapchainHandle::default(),
            renderer,
        }
    }
}

impl WindowEventHandler for Window {
    fn on_create(&mut self, _control: &mut dyn EventLoopControl, window_handle: WindowHandle) {
        let WindowHandle::Windows(hwnd) = window_handle;
        self.swapchain = self.renderer.borrow_mut().create_swapchain(hwnd).unwrap();
    }

    fn on_destroy(&mut self, _control: &mut dyn EventLoopControl) {
        self.renderer
            .borrow_mut()
            .destroy_swapchain(self.swapchain)
            .unwrap();
    }

    fn on_redraw(&mut self, _control: &mut dyn EventLoopControl, width: u32, height: u32) {
        if width > 0 && height > 0 {
            let mut renderer = self.renderer.borrow_mut();
            renderer.begin_frame(self.swapchain).unwrap();
            renderer
                .end_frame(self.swapchain, &TRIANGLE, &INDICES)
                .unwrap();
        }
    }

    fn on_mouse_move(&mut self, _control: &mut dyn EventLoopControl, new_x: i32, new_y: i32) {
        println!("{} {}", new_x, new_y);
    }

    fn on_mouse_button(
        &mut self,
        control: &mut dyn EventLoopControl,
        button: MouseButton,
        state: ButtonState,
    ) {
        match button {
            MouseButton::Left => match state {
                ButtonState::Down => {}
                ButtonState::Up => {
                    control.create_window(Box::new(Window::new(self.renderer.clone())));
                }
            },
            MouseButton::Right => {}
            MouseButton::Middle => {}
        }
        println!("{:?} {:?}", button, state);
    }
}
