use std::{cell::RefCell, rc::Rc};

use fathom::{
    geometry::{Extent, Point, Px},
    renderer::{Renderer, SwapchainHandle},
    shell::event_loop::{
        ButtonState, EventLoop, MouseButton, Proxy, WindowEventHandler, WindowHandle,
    },
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let renderer = Rc::new(RefCell::new(Renderer::new()?));

    let mut event_loop = EventLoop::new();
    event_loop.create_window(Box::new(Window::new(renderer)));
    event_loop.run();

    Ok(())
}

struct Window {
    window: Option<WindowHandle>,
    swapchain: SwapchainHandle,
    renderer: Rc<RefCell<Renderer>>,
}

impl Window {
    fn new(renderer: Rc<RefCell<Renderer>>) -> Self {
        Self {
            window: None,
            swapchain: SwapchainHandle::default(),
            renderer,
        }
    }
}

impl WindowEventHandler for Window {
    fn on_create(&mut self, window_handle: WindowHandle, _control: &mut dyn Proxy) {
        self.window = Some(window_handle);
        self.swapchain = self
            .renderer
            .borrow_mut()
            .create_swapchain(window_handle.raw())
            .unwrap();
    }

    fn on_close(&mut self, _control: &mut dyn Proxy) {}

    fn on_redraw(&mut self, _control: &mut dyn Proxy, window_size: Extent) {
        if window_size.width > Px(0) && window_size.height > Px(0) {
            // let mut renderer = self.renderer.borrow_mut();
            // renderer.begin_frame(self.swapchain)?;
            // renderer.end_frame(self.swapchain, Color::BLACK, ui.draw_commands())?;
        }
    }

    fn on_mouse_move(&mut self, _control: &mut dyn Proxy, _new_position: Point) {}

    fn on_mouse_button(
        &mut self,
        control: &mut dyn Proxy,
        button: MouseButton,
        state: ButtonState,
    ) {
        match button {
            MouseButton::Left => match state {
                ButtonState::Pressed => {}
                ButtonState::Released => {
                    control.create_window(Box::new(Window::new(self.renderer.clone())));
                }
            },
            MouseButton::Right => {
                if state == ButtonState::Released {
                    control.destroy_window(self.window.unwrap());
                }
            }
            MouseButton::Middle => {}
        }
    }
}

impl Drop for Window {
    fn drop(&mut self) {
        self.renderer
            .borrow_mut()
            .destroy_swapchain(self.swapchain)
            .unwrap();
    }
}
