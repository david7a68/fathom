use std::{cell::RefCell, rc::Rc};

use fathom::{
    color::Color,
    event_loop::{
        ButtonState, Control, EventLoop, EventReply, MouseButton, WindowEventHandler, WindowHandle,
    },
    geometry::{Extent, Point, Px},
    renderer::{Renderer, SwapchainHandle},
    ui::{ColorFill, Context},
};
use rand::random;

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
    ui_context: Context,
    do_once: bool,
}

impl Window {
    fn new(renderer: Rc<RefCell<Renderer>>) -> Self {
        Self {
            swapchain: SwapchainHandle::default(),
            renderer,
            ui_context: Context::new(
                Extent {
                    width: Px(0),
                    height: Px(0),
                },
                Color::BLUE,
            ),
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
        window_size: Extent,
    ) -> Result<EventReply, Box<dyn std::error::Error>> {
        if window_size.width > Px(0) && window_size.height > Px(0) {
            self.ui_context.update_size(window_size);

            let mut renderer = self.renderer.borrow_mut();
            renderer.begin_frame(self.swapchain)?;

            let ui = &mut self.ui_context;

            if !self.do_once {
                // ui.set_root(Box::new(XSplitPanel {
                //     panes: vec![
                //         (
                //             0.3,
                //             XSplitPane {
                //                 body: Box::new(ColorFill(Color::RED)),
                //             },
                //         ),
                //         (
                //             0.7,
                //             XSplitPane {
                //                 body: Box::new(ColorFill(Color::GREEN)),
                //             },
                //         ),
                //     ],
                // }));

                ui.set_root(Box::new(ColorFill(Color::RED)));

                self.do_once = true;
            }

            ui.update();

            renderer.end_frame(self.swapchain, Color::BLACK, ui.draw_commands())?;
        }

        Ok(EventReply::Continue)
    }

    fn on_mouse_move(
        &mut self,
        _control: &mut dyn Control,
        new_position: Point,
    ) -> Result<EventReply, Box<dyn std::error::Error>> {
        self.ui_context.update_cursor(new_position);
        Ok(EventReply::Continue)
    }

    fn on_mouse_button(
        &mut self,
        _control: &mut dyn Control,
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
