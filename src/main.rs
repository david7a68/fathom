mod renderer;

use windows::{
    core::PCWSTR,
    Win32::{
        Foundation::{GetLastError, HWND, LPARAM, LRESULT, WPARAM},
        System::LibraryLoader::GetModuleHandleW,
        UI::WindowsAndMessaging::{
            CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, LoadCursorW,
            PeekMessageW, PostQuitMessage, RegisterClassExW, ShowWindow, TranslateMessage,
            CS_HREDRAW, CS_VREDRAW, CW_USEDEFAULT, IDC_ARROW, MSG, PM_REMOVE, SW_SHOW,
            WINDOW_EX_STYLE, WM_DESTROY, WM_QUIT, WNDCLASSEXW, WS_OVERLAPPEDWINDOW,
        },
    },
};

use renderer::Renderer;

const WINDOW_TITLE: &str = "Hello!";

/// The name of Fathom's window classes `"FATHOM_WNDCLASS"` in UTF-16 as an
/// array of `u16`s.
const WNDCLASS_NAME: &[u16] = &[
    0x0046, 0x0041, 0x0054, 0x0048, 0x004f, 0x004d, 0x005f, 0x0057, 0x004e, 0x0044, 0x0043, 0x004c,
    0x0041, 0x0053, 0x0053,
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

    let mut renderer = Renderer::new();
    let swapchain = renderer.create_swapchain(hwnd, hinstance);

    unsafe { ShowWindow(hwnd, SW_SHOW) };

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

    renderer.destroy_swapchain(swapchain);

    Ok(())
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT::default()
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}
