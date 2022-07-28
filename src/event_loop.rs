use std::{cell::RefCell, rc::Rc};

use windows::{
    core::PCWSTR,
    Win32::{
        Foundation::{GetLastError, HWND, LPARAM, LRESULT, WPARAM},
        System::LibraryLoader::GetModuleHandleW,
        UI::WindowsAndMessaging::{
            CreateWindowExW, DefWindowProcW, DispatchMessageW, GetClientRect, GetMessageW,
            GetWindowLongPtrW, LoadCursorW, PeekMessageW, PostQuitMessage, RegisterClassExW,
            SetWindowLongPtrW, ShowWindow, TranslateMessage, CREATESTRUCTW, CS_HREDRAW, CS_VREDRAW,
            CW_USEDEFAULT, GWLP_USERDATA, IDC_ARROW, MSG, PM_REMOVE, SW_SHOW, WINDOW_EX_STYLE,
            WM_CREATE, WM_DESTROY, WM_ERASEBKGND, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MBUTTONDOWN,
            WM_MBUTTONUP, WM_MOUSEMOVE, WM_PAINT, WM_QUIT, WM_RBUTTONDOWN, WM_RBUTTONUP,
            WM_WINDOWPOSCHANGED, WNDCLASSEXW, WS_OVERLAPPEDWINDOW,
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

#[derive(Clone, Copy, Debug)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
}

#[derive(Clone, Copy, Debug)]
pub enum ButtonState {
    Down,
    Up,
}

pub enum WindowHandle {
    Windows(HWND),
}

pub trait WindowEventHandler {
    fn on_create(&mut self, event_loop: &mut dyn EventLoopControl, window_handle: WindowHandle);

    fn on_destroy(&mut self, event_loop: &mut dyn EventLoopControl);

    fn on_redraw(&mut self, event_loop: &mut dyn EventLoopControl, width: u32, height: u32);

    fn on_mouse_move(&mut self, event_loop: &mut dyn EventLoopControl, new_x: i32, new_y: i32);

    fn on_mouse_button(
        &mut self,
        event_loop: &mut dyn EventLoopControl,
        button: MouseButton,
        state: ButtonState,
    );
}

pub trait EventLoopControl {
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
            let arrow_cursor = unsafe { LoadCursorW(None, &IDC_ARROW) }.unwrap();

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

impl EventLoopControl for EventLoop {
    fn create_window(&mut self, window: Box<dyn WindowEventHandler>) {
        self.inner.create_window(window);
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

impl EventLoopControl for Rc<RefCell<EventLoopInner>> {
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

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if msg == WM_CREATE {
        let create_struct = lparam.0 as *const CREATESTRUCTW;
        let window = (*create_struct).lpCreateParams as *mut WindowData;

        (*window)
            .event_handler
            .on_create(&mut (*window).event_loop, WindowHandle::Windows(hwnd));
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, window as isize);

        return LRESULT(1);
    }

    let window = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut WindowData;

    if window.is_null() {
        return DefWindowProcW(hwnd, msg, wparam, lparam);
    }

    match msg {
        WM_DESTROY => {
            let mut window = Box::from_raw(window);
            window.event_handler.on_destroy(&mut window.event_loop);

            // If we only have two strong references, they must be window and
            // the event loop that owns it. Since this is the last window, it is
            // safe to exit the event loop.
            if Rc::strong_count(&(*window).event_loop) == 2 {
                PostQuitMessage(0);
            }

            LRESULT::default()
            // window is dropped on return
        }
        WM_WINDOWPOSCHANGED => LRESULT::default(),
        WM_ERASEBKGND => LRESULT(1),
        WM_PAINT => {
            let (width, height) = {
                let mut rect = std::mem::zeroed();
                GetClientRect(hwnd, &mut rect);
                (rect.right - rect.left, rect.bottom - rect.top)
            };
            (*window).event_handler.on_redraw(
                &mut (*window).event_loop,
                width as u32,
                height as u32,
            );
            LRESULT::default()
        }
        WM_MOUSEMOVE => {
            // cast to i16 preserves sign bit
            let x = lparam.0 as i16 as i32;
            let y = (lparam.0 >> 16) as i16 as i32;
            (*window)
                .event_handler
                .on_mouse_move(&mut (*window).event_loop, x, y);
            LRESULT::default()
        }
        WM_LBUTTONDOWN => {
            (*window).event_handler.on_mouse_button(
                &mut (*window).event_loop,
                MouseButton::Left,
                ButtonState::Down,
            );
            LRESULT::default()
        }
        WM_LBUTTONUP => {
            (*window).event_handler.on_mouse_button(
                &mut (*window).event_loop,
                MouseButton::Left,
                ButtonState::Up,
            );
            LRESULT::default()
        }
        WM_RBUTTONDOWN => {
            (*window).event_handler.on_mouse_button(
                &mut (*window).event_loop,
                MouseButton::Right,
                ButtonState::Down,
            );
            LRESULT::default()
        }
        WM_RBUTTONUP => {
            (*window).event_handler.on_mouse_button(
                &mut (*window).event_loop,
                MouseButton::Right,
                ButtonState::Up,
            );
            LRESULT::default()
        }
        WM_MBUTTONDOWN => {
            (*window).event_handler.on_mouse_button(
                &mut (*window).event_loop,
                MouseButton::Middle,
                ButtonState::Down,
            );
            LRESULT::default()
        }
        WM_MBUTTONUP => {
            (*window).event_handler.on_mouse_button(
                &mut (*window).event_loop,
                MouseButton::Middle,
                ButtonState::Up,
            );
            LRESULT::default()
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}
