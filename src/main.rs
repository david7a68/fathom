mod color;
mod indexed_store;
mod point;
mod renderer;

use std::{cell::RefCell, rc::Rc};

use color::Color;
use point::Point;
use windows::{
    core::PCWSTR,
    Win32::{
        Foundation::{GetLastError, HWND, LPARAM, LRESULT, WPARAM},
        System::LibraryLoader::GetModuleHandleW,
        UI::WindowsAndMessaging::{
            CreateWindowExW, DefWindowProcW, DispatchMessageW, GetClientRect, GetMessageW,
            GetWindowLongPtrW, LoadCursorW, PeekMessageW, PostQuitMessage, RegisterClassExW,
            SetWindowLongPtrW, ShowWindow, TranslateMessage, CS_HREDRAW, CS_VREDRAW, CW_USEDEFAULT,
            GWLP_USERDATA, IDC_ARROW, MSG, PM_REMOVE, SW_SHOW, WINDOW_EX_STYLE, WM_DESTROY,
            WM_ERASEBKGND, WM_PAINT, WM_QUIT, WM_WINDOWPOSCHANGED, WNDCLASSEXW,
            WS_OVERLAPPEDWINDOW,
        },
    },
};

use renderer::{Renderer, SwapchainHandle, Vertex};

const WINDOW_TITLE: &str = "Hello!";

/// The name of Fathom's window classes `"FATHOM_WNDCLASS"` in UTF-16 as an
/// array of `u16`s.
const WNDCLASS_NAME: &[u16] = &[
    0x0046, 0x0041, 0x0054, 0x0048, 0x004f, 0x004d, 0x005f, 0x0057, 0x004e, 0x0044, 0x0043, 0x004c,
    0x0041, 0x0053, 0x0053,
];

const TRIANGLE: [Vertex; 3] = [
    Vertex {
        point: Point { x: 0.0, y: -0.5 },
        color: Color {
            r: 1.0,
            g: 0.0,
            b: 0.0,
            a: 1.0,
        },
    },
    Vertex {
        point: Point { x: 0.5, y: 0.5 },
        color: Color {
            r: 0.0,
            g: 1.0,
            b: 0.0,
            a: 1.0,
        },
    },
    Vertex {
        point: Point { x: -0.5, y: 0.5 },
        color: Color {
            r: 0.0,
            g: 0.0,
            b: 1.0,
            a: 1.0,
        },
    },
];

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let hinstance = unsafe { GetModuleHandleW(None)? };

    let _wndclass_atom = {
        let arrow_cursor = unsafe { LoadCursorW(None, &IDC_ARROW)? };

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

    let hwnd = {
        let os_title = {
            use std::{ffi::OsStr, os::windows::prelude::OsStrExt};
            let mut buffer: Vec<u16> = OsStr::new(WINDOW_TITLE).encode_wide().collect();
            buffer.push(0);
            buffer
        };

        unsafe {
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
                std::ptr::null_mut(),
            )
        }
    };

    let renderer = Rc::new(RefCell::new(Renderer::new()?));
    let swapchain = renderer.borrow_mut().create_swapchain(hwnd, hinstance)?;

    let window = Box::new(Window {
        hwnd,
        renderer: renderer.clone(),
        swapchain,
    });

    unsafe {
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, Box::leak(window) as *const _ as _);
        ShowWindow(hwnd, SW_SHOW);
    }

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

    renderer.borrow_mut().destroy_swapchain(swapchain).unwrap();

    Ok(())
}

struct Window {
    hwnd: HWND,
    swapchain: SwapchainHandle,
    renderer: Rc<RefCell<Renderer>>,
}

impl Window {
    pub fn on_destroy(&mut self) {
        self.renderer
            .borrow_mut()
            .destroy_swapchain(self.swapchain)
            .unwrap();
    }

    pub fn on_redraw(&mut self) {
        let (width, height) = unsafe {
            let mut rect = std::mem::zeroed();
            GetClientRect(self.hwnd, &mut rect);
            (rect.right - rect.left, rect.bottom - rect.top)
        };

        if width > 0 && height > 0 {
            let mut renderer = self.renderer.borrow_mut();
            renderer.begin_frame(self.swapchain).unwrap();
            renderer.end_frame(self.swapchain, &TRIANGLE).unwrap();
        }
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let window = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Window;

    if window.is_null() {
        return DefWindowProcW(hwnd, msg, wparam, lparam);
    }

    if msg == WM_DESTROY {
        let mut window = Box::from_raw(window);
        window.on_destroy();
        PostQuitMessage(0);
        return LRESULT::default();
    }

    let window = &mut *window;

    match msg {
        WM_WINDOWPOSCHANGED => LRESULT::default(),
        WM_ERASEBKGND => LRESULT(1),
        WM_PAINT => {
            window.on_redraw();
            LRESULT::default()
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}
