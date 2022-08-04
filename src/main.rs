use std::{cell::RefCell, rc::Rc};

use fathom::{
    geometry::{Extent, Point, Px},
    renderer::{Renderer, SwapchainHandle},
    shell::event_loop::{
        ButtonState, Proxy, EventLoop, EventReply, MouseButton, WindowEventHandler, WindowHandle,
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
    fn on_create(
        &mut self,
        _control: &mut dyn Proxy,
        window_handle: WindowHandle,
    ) -> Result<EventReply, Box<dyn std::error::Error>> {
        let WindowHandle::Windows(hwnd) = window_handle;
        self.swapchain = self.renderer.borrow_mut().create_swapchain(hwnd).unwrap();
        Ok(EventReply::Continue)
    }

    fn on_close(
        &mut self,
        _control: &mut dyn Proxy,
    ) -> Result<EventReply, Box<dyn std::error::Error>> {
        Ok(EventReply::DestroyWindow)
    }

    fn on_redraw(
        &mut self,
        _control: &mut dyn Proxy,
        window_size: Extent,
    ) -> Result<EventReply, Box<dyn std::error::Error>> {
        if window_size.width > Px(0) && window_size.height > Px(0) {
            // let mut renderer = self.renderer.borrow_mut();
            // renderer.begin_frame(self.swapchain)?;
            // renderer.end_frame(self.swapchain, Color::BLACK, ui.draw_commands())?;
        }

        Ok(EventReply::Continue)
    }

    fn on_mouse_move(
        &mut self,
        _control: &mut dyn Proxy,
        _new_position: Point,
    ) -> Result<EventReply, Box<dyn std::error::Error>> {
        Ok(EventReply::Continue)
    }

    fn on_mouse_button(
        &mut self,
        _control: &mut dyn Proxy,
        button: MouseButton,
        state: ButtonState,
    ) -> Result<EventReply, Box<dyn std::error::Error>> {
        match button {
            MouseButton::Left => match state {
                ButtonState::Pressed => {}
                ButtonState::Released => {
                    // control.create_window(Box::new(Window::new(self.renderer.clone())));
                }
            },
            MouseButton::Right => {
                if state == ButtonState::Released {
                    return Ok(EventReply::DestroyWindow);
                }
            }
            MouseButton::Middle => {}
        }
        Ok(EventReply::Continue)
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
