use std::{cell::RefCell, rc::Rc};

use fathom::{
    color::Color,
    event_loop::{
        ButtonState, Control, EventLoop, EventReply, MouseButton, WindowEventHandler, WindowHandle,
    },
    renderer::{Renderer, SwapchainHandle, Vertex}, ui::Context,
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
    vertex_buffer: Vec<Vertex>,
    index_buffer: Vec<u16>,
    ui_context: Context,
    do_once: bool,
}

impl Window {
    fn new(renderer: Rc<RefCell<Renderer>>) -> Self {
        Self {
            swapchain: SwapchainHandle::default(),
            renderer,
            vertex_buffer: Vec::new(),
            index_buffer: Vec::new(),
            ui_context: Context::new(0, 0, Color::BLUE),
            do_once: false,
        }
    }
}

impl WindowEventHandler for Window {
    fn on_create(
        &mut self,
        _control: &mut dyn Control,
        window_handle: WindowHandle,
    ) -> Result<EventReply, Box<dyn std::error::Error>> {
        let WindowHandle::Windows(hwnd) = window_handle;
        self.swapchain = self.renderer.borrow_mut().create_swapchain(hwnd).unwrap();
        Ok(EventReply::Continue)
    }

    fn on_close(
        &mut self,
        _control: &mut dyn Control,
    ) -> Result<EventReply, Box<dyn std::error::Error>> {
        Ok(EventReply::DestroyWindow)
    }

    fn on_redraw(
        &mut self,
        _control: &mut dyn Control,
        width: u32,
        height: u32,
    ) -> Result<EventReply, Box<dyn std::error::Error>> {
        if width > 0 && height > 0 {
            self.ui_context.update_size(width, height);

            let mut renderer = self.renderer.borrow_mut();
            renderer.begin_frame(self.swapchain)?;

            let ui = &mut self.ui_context;

            if !self.do_once {
                let root = ui.root_panel();
                let (_left, _right) = ui.split_panel(root, 0.3);

                self.do_once = true;
            }

            ui.update();

            renderer.end_frame(self.swapchain, Color::BLACK, ui.vertex_buffer(), ui.index_buffer())?;
        }

        Ok(EventReply::Continue)
    }

    fn on_mouse_move(
        &mut self,
        _control: &mut dyn Control,
        _new_x: i32,
        _new_y: i32,
    ) -> Result<EventReply, Box<dyn std::error::Error>> {
        Ok(EventReply::Continue)
    }

    fn on_mouse_button(
        &mut self,
        control: &mut dyn Control,
        button: MouseButton,
        state: ButtonState,
    ) -> Result<EventReply, Box<dyn std::error::Error>> {
        match button {
            MouseButton::Left => match state {
                ButtonState::Pressed => {}
                ButtonState::Released => {
                    control.create_window(Box::new(Window::new(self.renderer.clone())));
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
