use std::{cell::RefCell, rc::Rc};

use windows::{
    core::PCWSTR,
    Win32::{
        Foundation::{GetLastError, HWND, LPARAM, LRESULT, WPARAM},
        System::LibraryLoader::GetModuleHandleW,
        UI::WindowsAndMessaging::{
            CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetClientRect,
            GetMessageW, GetWindowLongPtrW, LoadCursorW, PeekMessageW, PostQuitMessage,
            RegisterClassExW, SetWindowLongPtrW, ShowWindow, TranslateMessage, CREATESTRUCTW,
            CS_HREDRAW, CS_VREDRAW, CW_USEDEFAULT, GWLP_USERDATA, IDC_ARROW, MSG, PM_REMOVE,
            SW_SHOW, WINDOW_EX_STYLE, WM_CLOSE, WM_CREATE, WM_DESTROY, WM_ERASEBKGND,
            WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MBUTTONDOWN, WM_MBUTTONUP, WM_MOUSEMOVE, WM_PAINT,
            WM_QUIT, WM_RBUTTONDOWN, WM_RBUTTONUP, WM_WINDOWPOSCHANGED, WNDCLASSEXW,
            WS_OVERLAPPEDWINDOW,
        },
    },
};

const WINDOW_TITLE: &str = "Hello!";

/// The name of Fathom's window classes `"FATHOM_WNDCLASS"` in UTF-16 as an
/// array of `u16`s.
const WNDCLASS_NAME: &[u16] = &[
    0x0046, 0x0041, 0x0054, 0x0048, 0x004f, 0x004d, 0x005f, 0x0057, 0x004e, 0x0044, 0x0043, 0x004c,
    0x0041, 0x0053, 0x0053, 0,
];

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ButtonState {
    Pressed,
    Released,
}

#[derive(Clone, Copy, Debug)]
pub enum WindowHandle {
    Windows(HWND),
}

/// Deferred control of a window event loop. Use this to modify the lifetime of
/// the window.
///
/// IMPL(straivers, 2022-07-28): This design was adopted in order to permit
/// windows to be destroyed within their event handlers without introducing
/// re-entrancy. That is to say, calling `event_loop.destroy_window()` in an
/// event handler would prompt a call to `WindowEventHandler::on_destroy()`
/// whilst still within a different event handler. This is not possible with the
/// current design.
#[must_use]
#[derive(Default, Debug)]
pub enum EventReply {
    /// Continue processing the event loop. The window will remain open and
    /// accepting input.
    #[default]
    Continue,
    /// Destroy the window after the event handler returns. This will prompt a
    /// call to `WindowEventHandler::on_destroy()`.
    DestroyWindow,
}

pub trait WindowEventHandler {
    fn on_create(
        &mut self,
        control: &mut dyn Control,
        window_handle: WindowHandle,
    ) -> Result<EventReply, Box<dyn std::error::Error>>;

    fn on_close(
        &mut self,
        control: &mut dyn Control,
    ) -> Result<EventReply, Box<dyn std::error::Error>>;

    fn on_destroy(&mut self, control: &mut dyn Control) -> Result<(), Box<dyn std::error::Error>>;

    fn on_redraw(
        &mut self,
        control: &mut dyn Control,
        width: u32,
        height: u32,
    ) -> Result<EventReply, Box<dyn std::error::Error>>;

    fn on_mouse_move(
        &mut self,
        control: &mut dyn Control,
        new_x: i32,
        new_y: i32,
    ) -> Result<EventReply, Box<dyn std::error::Error>>;

    fn on_mouse_button(
        &mut self,
        control: &mut dyn Control,
        button: MouseButton,
        state: ButtonState,
    ) -> Result<EventReply, Box<dyn std::error::Error>>;
}

pub trait Control {
    fn create_window(&mut self, window: Box<dyn WindowEventHandler>);
}

struct WindowData {
    event_loop: Rc<RefCell<EventLoopInner>>,
    event_handler: Box<dyn WindowEventHandler>,
}

pub struct EventLoop {
    inner: Rc<RefCell<EventLoopInner>>,
}

impl EventLoop {
    pub fn new() -> Self {
        let hinstance = unsafe { GetModuleHandleW(None) }.unwrap();

        let _wndclass_atom = {
            let arrow_cursor = unsafe { LoadCursorW(None, IDC_ARROW) }.unwrap();

            let wndclass = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>().try_into().unwrap(),
                style: CS_VREDRAW | CS_HREDRAW,
                hInstance: hinstance,
                lpfnWndProc: Some(wndproc),
                lpszClassName: PCWSTR(WNDCLASS_NAME.as_ptr()),
                hCursor: arrow_cursor,
                ..WNDCLASSEXW::default()
            };

            unsafe { RegisterClassExW(&wndclass) }
        };

        Self {
            inner: Rc::new(RefCell::new(EventLoopInner {})),
        }
    }

    pub fn run(&mut self) {
        'event_pump: loop {
            let mut msg = MSG::default();

            if Rc::strong_count(&self.inner) == 1 {
                break 'event_pump;
            }

            let ret = unsafe { GetMessageW(&mut msg, None, 0, 0).0 };
            if ret == -1 {
                panic!("GetMessage failed. Error: {:?}", unsafe { GetLastError() });
            } else if ret == 0 {
                break;
            } else {
                unsafe {
                    TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }

            while unsafe { PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE) }.into() {
                if msg.message == WM_QUIT {
                    break 'event_pump;
                }

                unsafe {
                    TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
        }
    }
}

impl Control for EventLoop {
    fn create_window(&mut self, window: Box<dyn WindowEventHandler>) {
        self.inner.create_window(window);
    }
}

impl Default for EventLoop {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for EventLoop {
    fn drop(&mut self) {
        assert!(
            Rc::strong_count(&self.inner) == 1,
            "all windows must be destroyed before the event loop is dropped"
        );
    }
}

struct EventLoopInner {}

impl Control for Rc<RefCell<EventLoopInner>> {
    fn create_window(&mut self, window: Box<dyn WindowEventHandler>) {
        let hinstance = unsafe { GetModuleHandleW(None) }.unwrap();

        let os_title = {
            use std::{ffi::OsStr, os::windows::prelude::OsStrExt};
            let mut buffer: Vec<u16> = OsStr::new(WINDOW_TITLE).encode_wide().collect();
            buffer.push(0);
            buffer
        };

        let window = Box::into_raw(Box::new(WindowData {
            event_loop: self.clone(),
            event_handler: window,
        }));

        let hwnd = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                PCWSTR(WNDCLASS_NAME.as_ptr()),
                PCWSTR(os_title.as_ptr()),
                WS_OVERLAPPEDWINDOW,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                None,
                None,
                hinstance,
                window.cast(),
            )
        };

        unsafe { ShowWindow(hwnd, SW_SHOW) };
    }
}

fn handle_event_reply(window: HWND, reply: Result<EventReply, Box<dyn std::error::Error>>) {
    match reply {
        Ok(EventReply::Continue) => (),
        Ok(EventReply::DestroyWindow) => unsafe {
            DestroyWindow(window);
        },
        Err(e) => {
            println!("An error occurred while handling a window event: {}", e);
            unsafe {
                DestroyWindow(window);
            }
        }
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if msg == WM_CREATE {
        let create_struct = lparam.0 as *const CREATESTRUCTW;
        let window = (*create_struct).lpCreateParams.cast::<WindowData>();

        handle_event_reply(
            hwnd,
            (*window)
                .event_handler
                .on_create(&mut (*window).event_loop, WindowHandle::Windows(hwnd)),
        );
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, window as isize);

        return LRESULT(1);
    }

    let window = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut WindowData;

    if window.is_null() {
        return DefWindowProcW(hwnd, msg, wparam, lparam);
    }

    let event_handler = &mut (*window).event_handler;
    let control = &mut (*window).event_loop;

    let reply = match msg {
        WM_CLOSE => event_handler.on_close(control),
        WM_PAINT => {
            let (width, height) = {
                let mut rect = std::mem::zeroed();
                GetClientRect(hwnd, &mut rect);
                (rect.right - rect.left, rect.bottom - rect.top)
            };
            event_handler.on_redraw(control, width as u32, height as u32)
        }
        WM_MOUSEMOVE => {
            // cast to i16 preserves sign bit
            let x = i32::from(lparam.0 as i16);
            let y = i32::from((lparam.0 >> 16) as i16);
            event_handler.on_mouse_move(control, x, y)
        }
        WM_LBUTTONDOWN => {
            event_handler.on_mouse_button(control, MouseButton::Left, ButtonState::Pressed)
        }
        WM_LBUTTONUP => {
            event_handler.on_mouse_button(control, MouseButton::Left, ButtonState::Released)
        }
        WM_RBUTTONDOWN => {
            event_handler.on_mouse_button(control, MouseButton::Right, ButtonState::Pressed)
        }
        WM_RBUTTONUP => {
            event_handler.on_mouse_button(control, MouseButton::Right, ButtonState::Released)
        }
        WM_MBUTTONDOWN => {
            event_handler.on_mouse_button(control, MouseButton::Middle, ButtonState::Pressed)
        }
        WM_MBUTTONUP => {
            event_handler.on_mouse_button(control, MouseButton::Middle, ButtonState::Released)
        }
        special_return => {
            return match special_return {
                WM_DESTROY => {
                    if let Err(e) = event_handler.on_destroy(control) {
                        println!("An error occurred while destroying a window: {}", e);
                    }

                    std::mem::drop(Box::from_raw(window));

                    // If we only have one strong reference, it must be owned by the
                    // event loop and there are no more windows to source events from.
                    // In this case, exit the event loop.
                    if Rc::strong_count(control) == 1 {
                        PostQuitMessage(0);
                    }
                    LRESULT(0)
                }
                WM_WINDOWPOSCHANGED => LRESULT(0),
                WM_ERASEBKGND => LRESULT(1),
                _ => DefWindowProcW(hwnd, msg, wparam, lparam),
            };
        }
    };

    handle_event_reply(hwnd, reply);

    LRESULT(0)
}
