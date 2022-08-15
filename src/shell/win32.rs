use crate::geometry::{Extent, Point, Px};

use std::{cell::RefCell, rc::Rc};
use windows::{
    core::PCWSTR,
    Win32::{
        Foundation::{GetLastError, HWND, LPARAM, LRESULT, WPARAM},
        System::LibraryLoader::GetModuleHandleW,
        UI::WindowsAndMessaging::{
            CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW,
            GetWindowLongPtrW, LoadCursorW, PeekMessageW, PostMessageW, PostQuitMessage,
            RegisterClassExW, SetWindowLongPtrW, ShowWindow, TranslateMessage, CREATESTRUCTW,
            CS_HREDRAW, CS_VREDRAW, CW_USEDEFAULT, GWLP_USERDATA, IDC_ARROW, MSG, PM_REMOVE,
            SW_SHOW, WINDOW_EX_STYLE, WM_CLOSE, WM_CREATE, WM_DESTROY, WM_ERASEBKGND,
            WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MBUTTONDOWN, WM_MBUTTONUP, WM_MOUSEMOVE, WM_PAINT,
            WM_QUIT, WM_RBUTTONDOWN, WM_RBUTTONUP, WM_SIZE, WM_USER, WM_WINDOWPOSCHANGED,
            WNDCLASSEXW, WS_OVERLAPPEDWINDOW,
        },
    },
};

use super::{ButtonState, MouseButton, Proxy, WindowConfig, WindowEventHandler};

const WINDOW_TITLE: &str = "Hello!";

const UM_DESTROY_WINDOW: u32 = WM_USER + 1;

/// The name of Fathom's window classes `"FATHOM_WNDCLASS"` in UTF-16 as an
/// array of `u16`s.
const WNDCLASS_NAME: &[u16] = &[
    0x0046, 0x0041, 0x0054, 0x0048, 0x004f, 0x004d, 0x005f, 0x0057, 0x004e, 0x0044, 0x0043, 0x004c,
    0x0041, 0x0053, 0x0053, 0,
];

#[derive(Clone, Copy, Debug)]
pub struct WindowHandle(HWND);

impl WindowHandle {
    #[cfg(target_os = "windows")]
    pub fn raw(&self) -> HWND {
        self.0
    }
}

/// Window-specific data that is associated with each window.
///
/// This is kept behind a `Box` to permit it to be associated with Windows'
/// windows through the `SetWindowLongPtr()`/`GetWindowLongPtr()` API.
struct WindowData {
    extent: Extent,
    /// A reference to data shared between every window. The refcount is used to
    /// explicitly track the number of windows that are currently open. There
    /// are no open windows if the refcount is 1, since one reference is
    /// maintained by `EventLoop`.
    event_loop: Rc<RefCell<EventLoopInner>>,
    /// A pointer to the window event handler.
    event_handler: Box<dyn WindowEventHandler>,
}

impl WindowData {
    fn wndproc(&mut self, hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        match msg {
            WM_CREATE => {
                self.event_handler
                    .on_create(WindowHandle(hwnd), &mut self.event_loop);
            }
            WM_CLOSE => self.event_handler.on_close(&mut self.event_loop),
            WM_PAINT => self
                .event_handler
                .on_redraw(&mut self.event_loop, self.extent),
            WM_SIZE => {
                let width = lparam.0 as i16;
                let height = (lparam.0 >> 16) as i16;
                self.extent = Extent {
                    width: Px(width),
                    height: Px(height),
                };
            }
            WM_MOUSEMOVE => {
                let x = Px(lparam.0 as i16);
                let y = Px((lparam.0 >> 16) as i16);
                self.event_handler
                    .on_mouse_move(&mut self.event_loop, Point { x, y });
            }
            WM_LBUTTONDOWN => self.event_handler.on_mouse_button(
                &mut self.event_loop,
                MouseButton::Left,
                ButtonState::Pressed,
            ),
            WM_LBUTTONUP => self.event_handler.on_mouse_button(
                &mut self.event_loop,
                MouseButton::Left,
                ButtonState::Released,
            ),
            WM_RBUTTONDOWN => self.event_handler.on_mouse_button(
                &mut self.event_loop,
                MouseButton::Right,
                ButtonState::Pressed,
            ),
            WM_RBUTTONUP => self.event_handler.on_mouse_button(
                &mut self.event_loop,
                MouseButton::Right,
                ButtonState::Released,
            ),
            WM_MBUTTONDOWN => self.event_handler.on_mouse_button(
                &mut self.event_loop,
                MouseButton::Middle,
                ButtonState::Pressed,
            ),
            WM_MBUTTONUP => self.event_handler.on_mouse_button(
                &mut self.event_loop,
                MouseButton::Middle,
                ButtonState::Released,
            ),
            special_return => {
                return match special_return {
                    WM_WINDOWPOSCHANGED => LRESULT(0),
                    WM_ERASEBKGND => LRESULT(1),
                    _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
                };
            }
        }

        LRESULT(0)
    }
}

pub struct EventLoop {
    inner: Rc<RefCell<EventLoopInner>>,
}

impl EventLoop {
    /// Initializes the event loop.
    pub fn new() -> Self {
        let hinstance = unsafe { GetModuleHandleW(None) }.unwrap();

        let _wndclass_atom = {
            let arrow_cursor = unsafe { LoadCursorW(None, IDC_ARROW) }.unwrap();

            let wndclass = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>().try_into().unwrap(),
                style: CS_VREDRAW | CS_HREDRAW,
                hInstance: hinstance,
                lpfnWndProc: Some(unsafe_wndproc),
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

    pub fn create_window(&self, config: &WindowConfig, window: Box<dyn WindowEventHandler>) {
        self.inner.create_window(config, window);
    }

    /// Runs the event loop until there are no windows open.
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

impl Drop for EventLoop {
    fn drop(&mut self) {
        assert!(
            Rc::strong_count(&self.inner) == 1,
            "all windows must be destroyed before the event loop is dropped"
        );
    }
}

struct EventLoopInner {}

impl Proxy for Rc<RefCell<EventLoopInner>> {
    fn create_window(&self, config: &WindowConfig, window: Box<dyn WindowEventHandler>) {
        let hinstance = unsafe { GetModuleHandleW(None) }.unwrap();

        let os_title = {
            use std::{ffi::OsStr, os::windows::prelude::OsStrExt};
            let mut buffer: Vec<u16> = OsStr::new(config.title).encode_wide().collect();
            buffer.push(0);
            buffer
        };

        let window = Box::into_raw(Box::new(WindowData {
            event_loop: self.clone(),
            event_handler: window,
            extent: Extent::default(),
        }));

        let (width, height) = if let Some(extent) = config.extent {
            (extent.width.0.into(), extent.height.0.into())
        } else {
            (CW_USEDEFAULT, CW_USEDEFAULT)
        };

        let hwnd = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                PCWSTR(WNDCLASS_NAME.as_ptr()),
                PCWSTR(os_title.as_ptr()),
                WS_OVERLAPPEDWINDOW,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                width,
                height,
                None,
                None,
                hinstance,
                window.cast(),
            )
        };

        unsafe { ShowWindow(hwnd, SW_SHOW) };
    }

    fn destroy_window(&self, window: WindowHandle) {
        unsafe {
            PostMessageW(window.raw(), UM_DESTROY_WINDOW, WPARAM(0), LPARAM(0));
        }
    }
}

unsafe extern "system" fn unsafe_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if msg == WM_CREATE {
        let create_struct = lparam.0 as *const CREATESTRUCTW;
        let window = (*create_struct).lpCreateParams.cast::<WindowData>();
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, window as isize);
    }

    let window = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut WindowData;

    if window.is_null() {
        DefWindowProcW(hwnd, msg, wparam, lparam)
    } else {
        match msg {
            WM_DESTROY => {
                std::mem::drop(Box::from_raw(window));
                if Rc::strong_count(&(*window).event_loop) == 1 {
                    PostQuitMessage(0);
                }
                LRESULT(0)
            }
            UM_DESTROY_WINDOW => {
                DestroyWindow(hwnd);
                LRESULT(0)
            }
            rest => (*window).wndproc(hwnd, rest, wparam, lparam),
        }
    }
}
