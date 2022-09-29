use std::collections::HashMap;

use crate::{
    gfx::geometry::Extent,
    gui::{
        input::{ButtonState, Input, MouseButton},
        widgets::{DrawContext, LayoutContext, UpdateContext, Widget},
    },
    renderer::{Renderer, SwapchainHandle},
    shell::{
        event::{Event, Window as WindowEvent},
        {OsShell, Shell, WindowConfig, WindowId},
    },
};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("an internal renderer error occurred")]
    Renderer(#[from] crate::renderer::Error),
}

pub struct AppWindowConfig<'a> {
    pub title: &'a str,
    pub extent: Option<Extent>,
    pub widget_tree: Box<dyn Widget>,
}

pub struct Application {}

impl Application {
    pub fn new() -> Result<Self, Error> {
        Ok(Self {})
    }

    pub fn run(&mut self, configs: Vec<AppWindowConfig>) {
        let shell = OsShell::initialize();
        let mut renderer = Renderer::new().unwrap();

        // TODO(straivers): for efficiency, we really should find a way to bind
        // AppWindow to the HWND directly.
        let mut windows = HashMap::<WindowId, AppWindow>::new();

        for config in configs {
            let window_id = shell
                .create_window(&WindowConfig {
                    title: config.title,
                    extent: config.extent,
                })
                .unwrap();

            let swapchain = renderer.create_swapchain(shell.hwnd(window_id)).unwrap();

            windows.insert(
                window_id,
                AppWindow {
                    swapchain,
                    extent: Extent::zero(),
                    input: Input::default(),
                    widget_tree: config.widget_tree,
                    needs_repaint: true,
                },
            );
        }

        shell.run_event_loop(move |event, shell, control| {
            control.wait();

            match event {
                Event::None => {}
                Event::Window { window_id, event } => {
                    let window = windows.get_mut(&window_id).unwrap_or_else(|| {
                        panic!(
                            "could not find window {:?} for event {:?}",
                            window_id, event
                        )
                    });

                    match event {
                        WindowEvent::Init { inner_extent } => {
                            window.extent = inner_extent;
                            window.needs_repaint = true;
                            shell.show_window(window_id);
                        }
                        WindowEvent::Destroyed => {
                            let window = windows.remove(&window_id).unwrap();
                            renderer.destroy_swapchain(window.swapchain).unwrap();
                            std::mem::drop(window);
                            control.exit();
                        }
                        WindowEvent::CloseRequested => {
                            shell.destroy_window(window_id);
                        }
                        WindowEvent::Resized { inner_extent } => {
                            window.extent = inner_extent;
                            window.needs_repaint = true;
                        }
                        WindowEvent::CursorMoved { position } => {
                            window.input.update_cursor_position(position);
                            window.needs_repaint |= UpdateContext::new(&window.input)
                                .begin(window.widget_tree.as_mut());
                        }
                        WindowEvent::Repaint => {
                            if window.needs_repaint {
                                LayoutContext::default()
                                    .begin(window.widget_tree.as_mut(), window.extent);

                                let mut canvas = renderer.begin_frame(window.swapchain).unwrap();
                                let mut draw_context = DrawContext::new(&mut canvas);
                                draw_context.draw(window.widget_tree.as_ref());
                                renderer.submit(canvas).unwrap();
                                window.needs_repaint = false;
                            }
                        }
                        WindowEvent::LeftMouseButtonPressed => {
                            window
                                .input
                                .update_mouse_button(MouseButton::Left, ButtonState::Pressed);

                            window.needs_repaint |= UpdateContext::new(&window.input)
                                .begin(window.widget_tree.as_mut());
                        }
                        WindowEvent::LeftMouseButtonReleased => {
                            window
                                .input
                                .update_mouse_button(MouseButton::Left, ButtonState::Released);

                            window.needs_repaint |= UpdateContext::new(&window.input)
                                .begin(window.widget_tree.as_mut());
                        }
                        WindowEvent::RightMouseButtonPressed => {
                            window
                                .input
                                .update_mouse_button(MouseButton::Right, ButtonState::Pressed);

                            window.needs_repaint |= UpdateContext::new(&window.input)
                                .begin(window.widget_tree.as_mut());
                        }
                        WindowEvent::RightMouseButtonReleased => {
                            window
                                .input
                                .update_mouse_button(MouseButton::Right, ButtonState::Released);

                            window.needs_repaint |= UpdateContext::new(&window.input)
                                .begin(window.widget_tree.as_mut());
                        }
                        WindowEvent::MiddleMouseButtonPressed => {
                            window
                                .input
                                .update_mouse_button(MouseButton::Middle, ButtonState::Pressed);

                            window.needs_repaint |= UpdateContext::new(&window.input)
                                .begin(window.widget_tree.as_mut());
                        }
                        WindowEvent::MiddleMouseButtonReleased => {
                            window
                                .input
                                .update_mouse_button(MouseButton::Middle, ButtonState::Released);

                            window.needs_repaint |= UpdateContext::new(&window.input)
                                .begin(window.widget_tree.as_mut());
                        }
                    }
                }
                Event::RepaintComplete => {
                    // flipping swapchains goes here!
                }
            }
        });
    }
}

struct AppWindow {
    extent: Extent,
    swapchain: SwapchainHandle,
    input: Input,
    widget_tree: Box<dyn Widget>,
    needs_repaint: bool,
}

// impl WindowEventHandler for AppWindow {
//     fn on_create(&mut self, window_handle: WindowHandle, extent: Extent, _control: &mut dyn Proxy) {
//         self.handle = Some(window_handle);
//         self.swapchain = self
//             .renderer
//             .borrow_mut()
//             .create_swapchain(window_handle.raw())
//             .unwrap();

//         LayoutContext::default().begin(self.widget_tree.as_mut(), extent);
//         UpdateContext::new(&self.input).update(self.widget_tree.as_mut());
//     }

//     fn on_close(&mut self, control: &mut dyn Proxy) {
//         control.destroy_window(self.handle.unwrap());
//     }

//     fn on_redraw(&mut self, _control: &mut dyn Proxy, window_size: Extent) {
//         self.input.tick();

//         if window_size != Extent::zero() {
//             LayoutContext::default().begin(self.widget_tree.as_mut(), window_size);

//             let mut renderer = self.renderer.borrow_mut();
//             let mut canvas = renderer.begin_frame(self.swapchain).unwrap();
//             let mut draw_context = DrawContext::new(&mut canvas);
//             draw_context.draw(self.widget_tree.as_ref());
//             renderer.submit(canvas).unwrap();
//         }
//     }

//     fn on_mouse_move(&mut self, _control: &mut dyn Proxy, new_position: Point) {
//         self.input.update_cursor_position(new_position);
//         UpdateContext::new(&self.input).update(self.widget_tree.as_mut());
//     }

//     fn on_mouse_button(
//         &mut self,
//         _control: &mut dyn Proxy,
//         button: MouseButton,
//         state: ButtonState,
//     ) {
//         self.input.update_mouse_button(button, state);
//         UpdateContext::new(&self.input).update(self.widget_tree.as_mut());
//     }
// }

// impl Drop for AppWindow {
//     fn drop(&mut self) {
//         self.renderer
//             .borrow_mut()
//             .destroy_swapchain(self.swapchain)
//             .unwrap();
//     }
// }
