use std::{cell::RefCell, rc::Rc};

use crate::{
    geometry::Extent,
    renderer::{Renderer, SwapchainHandle},
    shell::event_loop::{
        ButtonState, EventLoop, MouseButton, Proxy, WindowEventHandler, WindowHandle,
    shell::{
        event_loop::{EventLoop, Proxy, WindowEventHandler, WindowHandle},
        input::{ButtonState, MouseButton},
    },
};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("an internal renderer error occurred")]
    Renderer(#[from] crate::renderer::Error),
}

pub struct WindowConfig<'a> {
    pub title: &'a str,
    pub extent: Option<Extent>,
    pub ui_builder: &'a dyn Fn(),
}

impl<'a> WindowConfig<'a> {
    fn shell_config(&self) -> crate::shell::event_loop::WindowConfig {
        crate::shell::event_loop::WindowConfig {
            title: self.title,
            extent: self.extent,
        }
    }
}

pub struct Application {
    renderer: Rc<RefCell<Renderer>>,
    event_loop: EventLoop,
}

impl Application {
    pub fn new() -> Result<Self, Error> {
        Ok(Self {
            renderer: Rc::new(RefCell::new(Renderer::new()?)),
            event_loop: EventLoop::new(),
        })
    }

    pub fn run(&mut self, windows: &[WindowConfig]) {
        for config in windows {
            self.event_loop.create_window(
                &config.shell_config(),
                Box::new(AppWindow::new(self.renderer.clone())),
            );
        }

        self.event_loop.run();
    }
}

struct AppWindow {
    handle: Option<WindowHandle>,
    swapchain: SwapchainHandle,
    renderer: Rc<RefCell<Renderer>>,
}

impl AppWindow {
    pub fn new(renderer: Rc<RefCell<Renderer>>) -> Self {
        Self {
            handle: None,
            swapchain: SwapchainHandle::default(),
            renderer,
        }
    }
}

impl WindowEventHandler for AppWindow {
    fn on_create(&mut self, window_handle: WindowHandle, control: &mut dyn Proxy) {
        self.handle = Some(window_handle);
        self.swapchain = self
            .renderer
            .borrow_mut()
            .create_swapchain(window_handle.raw())
            .unwrap();
    }

    fn on_close(&mut self, control: &mut dyn Proxy) {
        control.destroy_window(self.handle.unwrap());
    }

    fn on_redraw(&mut self, control: &mut dyn Proxy, window_size: Extent) {
        // no-op
    }

    fn on_mouse_move(&mut self, control: &mut dyn Proxy, new_position: crate::geometry::Point) {
        // no-op
    }

    fn on_mouse_button(
        &mut self,
        control: &mut dyn Proxy,
        button: MouseButton,
        state: ButtonState,
    ) {
        match button {
            MouseButton::Left => match state {
                ButtonState::Released => {}
                ButtonState::Pressed => {
                    control.create_window(
                        &WindowConfig {
                            title: &format!("Window #{}", Rc::strong_count(&self.renderer)),
                            extent: None,
                            ui_builder: &|| {},
                        }
                        .shell_config(),
                        Box::new(AppWindow::new(self.renderer.clone())),
                    );
                }
            },
            MouseButton::Right => match state {
                ButtonState::Released => {}
                ButtonState::Pressed => {
                    control.destroy_window(self.handle.unwrap());
                }
            },
            MouseButton::Middle => match state {
                ButtonState::Released => {}
                ButtonState::Pressed => {}
            },
        }
    }
}

impl Drop for AppWindow {
    fn drop(&mut self) {
        self.renderer
            .borrow_mut()
            .destroy_swapchain(self.swapchain)
            .unwrap();
    }
}
