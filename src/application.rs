use std::{cell::RefCell, rc::Rc};

use crate::{
    color::Color,
    geometry::{Extent, Point},
    gui::widgets::{Canvas, LayoutContext, UpdateContext, Widget},
    renderer::{Renderer, SwapchainHandle},
    shell::{
        event_loop::{EventLoop, Proxy, WindowEventHandler, WindowHandle},
        input::{ButtonState, Input, MouseButton},
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
    pub widget_tree: Box<dyn Widget>,
}

impl<'a> WindowConfig<'a> {
    fn destructure(self) -> (crate::shell::event_loop::WindowConfig<'a>, Box<dyn Widget>) {
        (
            crate::shell::event_loop::WindowConfig {
                title: self.title,
                extent: self.extent,
            },
            self.widget_tree,
        )
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

    pub fn run(&mut self, windows: Vec<WindowConfig>) {
        for config in windows {
            let (window_config, widget_tree) = config.destructure();
            self.event_loop.create_window(
                &window_config,
                Box::new(AppWindow::new(self.renderer.clone(), widget_tree)),
            );
        }

        self.event_loop.run();
    }
}

struct AppWindow {
    handle: Option<WindowHandle>,
    swapchain: SwapchainHandle,
    renderer: Rc<RefCell<Renderer>>,
    input: Input,
    widget_tree: Box<dyn Widget>,
}

impl AppWindow {
    pub fn new(renderer: Rc<RefCell<Renderer>>, widget_tree: Box<dyn Widget>) -> Self {
        Self {
            handle: None,
            swapchain: SwapchainHandle::default(),
            renderer,
            input: Input::default(),
            widget_tree,
        }
    }
}

impl WindowEventHandler for AppWindow {
    fn on_create(&mut self, window_handle: WindowHandle, extent: Extent, _control: &mut dyn Proxy) {
        self.handle = Some(window_handle);
        self.swapchain = self
            .renderer
            .borrow_mut()
            .create_swapchain(window_handle.raw())
            .unwrap();

        LayoutContext::default().begin(self.widget_tree.as_mut(), extent);
        UpdateContext::new(&self.input).update(self.widget_tree.as_mut());
    }

    fn on_close(&mut self, control: &mut dyn Proxy) {
        control.destroy_window(self.handle.unwrap());
    }

    fn on_redraw(&mut self, _control: &mut dyn Proxy, window_size: Extent) {
        self.input.tick();

        if window_size != Extent::zero() {
            LayoutContext::default().begin(self.widget_tree.as_mut(), window_size);

            let mut canvas = Canvas::default();
            canvas.draw(self.widget_tree.as_ref());
            let command_buffer = canvas.finish();

            let mut renderer = self.renderer.borrow_mut();
            renderer.begin_frame(self.swapchain).unwrap();
            renderer
                .end_frame(self.swapchain, Color::BLACK, &command_buffer)
                .unwrap();
        }
    }

    fn on_mouse_move(&mut self, _control: &mut dyn Proxy, new_position: Point) {
        self.input.update_cursor_position(new_position);
        UpdateContext::new(&self.input).update(self.widget_tree.as_mut());
    }

    fn on_mouse_button(
        &mut self,
        _control: &mut dyn Proxy,
        button: MouseButton,
        state: ButtonState,
    ) {
        self.input.update_mouse_button(button, state);
        UpdateContext::new(&self.input).update(self.widget_tree.as_mut());
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
