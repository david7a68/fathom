use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    thread::ThreadId,
};

use once_cell::sync::OnceCell;
use windows::{
    core::PCWSTR,
    Win32::{
        Foundation::{GetLastError, HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM},
        Graphics::Gdi::{BeginPaint, EndPaint, PAINTSTRUCT},
        System::LibraryLoader::GetModuleHandleW,
        UI::WindowsAndMessaging::{
            CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetClientRect,
            GetMessageW, GetWindowLongPtrW, LoadCursorW, PeekMessageW, PostMessageW,
            PostQuitMessage, RegisterClassExW, SetWindowLongPtrW, ShowWindow, TranslateMessage,
            CREATESTRUCTW, CS_HREDRAW, CS_VREDRAW, CW_USEDEFAULT, GWLP_USERDATA, IDC_ARROW, MSG,
            PM_REMOVE, SWP_NOCOPYBITS, SW_HIDE, SW_SHOW, WINDOWPOS, WINDOW_EX_STYLE, WM_CLOSE,
            WM_CREATE, WM_DESTROY, WM_ERASEBKGND, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MBUTTONDOWN,
            WM_MBUTTONUP, WM_MOUSEMOVE, WM_PAINT, WM_QUIT, WM_RBUTTONDOWN, WM_RBUTTONUP, WM_USER,
            WM_WINDOWPOSCHANGED, WM_WINDOWPOSCHANGING, WNDCLASSEXW, WS_OVERLAPPEDWINDOW,
        },
    },
};

use crate::{
    gfx::geometry::{Extent, Point, Px},
    shell::event::{Event, Window as WindowEvent},
};

use super::{Error, EventLoopControl, WindowConfig};

/// This message is sent when the user destroys a window (by dropping the
/// window) instead of calling `DestroyWindow` in order to avoid re-entrancy in
/// the event loop. It will be enqueued at the end of the message buffer and
/// called once execution is outside of the event callback.
const UM_DESTROY_WINDOW: u32 = WM_USER + 1;

/// The name of Fathom's window classes `"FATHOM_WNDCLASS"` in UTF-16 as an
/// array of `u16`s.
const WNDCLASS_NAME: &[u16] = &[
    0x0046, 0x0041, 0x0054, 0x0048, 0x004f, 0x004d, 0x005f, 0x0057, 0x004e, 0x0044, 0x0043, 0x004c,
    0x0041, 0x0053, 0x0053, 0,
];

#[derive(Clone, Copy, Debug, Eq)]
pub struct WindowId {
    hwnd: HWND,
}

impl PartialEq for WindowId {
    fn eq(&self, other: &Self) -> bool {
        self.hwnd == other.hwnd
    }
}

impl std::hash::Hash for WindowId {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.hwnd.0.hash(state);
    }
}

impl From<HWND> for super::WindowId {
    fn from(hwnd: HWND) -> Self {
        super::WindowId(WindowId { hwnd })
    }
}

static SHELL_THREAD: OnceCell<ThreadId> = OnceCell::new();
static RUNNING: OnceCell<bool> = OnceCell::new();

pub struct OsShell {
    inner: Rc<Inner>,
}

impl OsShell {
    pub fn initialize() -> Self {
        SHELL_THREAD.set(std::thread::current().id()).expect(
            "Only one instance of the shell may be initialized for the lifetime of the program",
        );

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
            inner: Rc::new(Inner {
                hinstance,
                init_event_buffer: RefCell::new(vec![]),
                windows: RefCell::new(vec![]),
                is_shutting_down: Cell::new(false),
                event_mode: Cell::new(EventLoopControl::Poll),
                event_callback: RefCell::new(None),
            }),
        }
    }

    /// NOTE(straivers): this function is a little bit not good because it needs
    /// access to `super::ShellProxy` and indeed it to the event handling code.
    /// Unfortunately, this is necessary to preserve the illusion that
    /// `super::ShellProxy` actually does something useful instead of wrapping
    /// `self::ShellProxy`, which is itself not terribly useful.
    pub fn run_event_loop<F>(&self, callback: F) -> !
    where
        F: 'static + FnMut(Event, &dyn super::Shell, &mut EventLoopControl),
    {
        RUNNING
            .set(true)
            .expect("run_event_loop can only be called once");

        *self.inner.event_callback.borrow_mut() = Some(Box::new(callback));

        {
            let buffered_events = self.inner.init_event_buffer.take();
            dispatch(&self.inner, buffered_events);
        }

        'evt: loop {
            let mode = self.inner.event_mode.get();

            if mode == EventLoopControl::Exit {
                break;
            }

            let mut msg = MSG::default();
            if mode == EventLoopControl::Wait {
                match unsafe { GetMessageW(&mut msg, None, 0, 0).0 } {
                    -1 => panic!("GetMessage failed. Error: {:?}", unsafe { GetLastError() }),
                    0 => break 'evt,
                    _ => unsafe {
                        TranslateMessage(&msg);
                        DispatchMessageW(&msg);
                    },
                }
            }

            while unsafe { PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE) }.into() {
                if msg.message == WM_QUIT {
                    break 'evt;
                }

                unsafe {
                    TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }

            dispatch(
                &self.inner,
                self.inner
                    .windows
                    .borrow()
                    .iter()
                    .map(|hwnd| Event::Window {
                        window_id: (*hwnd).into(),
                        event: WindowEvent::Repaint,
                    })
                    .chain(std::iter::once(Event::RepaintComplete)),
            );
        }

        clean_exit(&self.inner);
    }
}

impl super::Shell for OsShell {
    fn create_window(&self, config: &WindowConfig) -> Result<super::WindowId, Error> {
        self.inner.create_window(config)
    }

    fn destroy_window(&self, window: super::WindowId) {
        self.inner.destroy_window(window);
    }

    fn show_window(&self, window: super::WindowId) {
        self.inner.show_window(window);
    }

    fn hide_window(&self, window: super::WindowId) {
        self.inner.hide_window(window);
    }

    fn hwnd(&self, window: super::WindowId) -> windows::Win32::Foundation::HWND {
        self.inner.hwnd(window)
    }
}

type InnerPtr = *const Inner;

pub(super) struct Inner {
    hinstance: HINSTANCE,
    init_event_buffer: RefCell<Vec<Event>>,
    /// A simple array used to keep track of every currently open window.
    windows: RefCell<Vec<HWND>>,
    is_shutting_down: Cell<bool>,
    event_mode: Cell<EventLoopControl>,
    #[allow(clippy::type_complexity)]
    event_callback:
        RefCell<Option<Box<dyn FnMut(Event, &dyn super::Shell, &mut EventLoopControl)>>>,
}

impl super::Shell for Rc<Inner> {
    fn create_window(&self, config: &WindowConfig) -> Result<super::WindowId, Error> {
        let hinstance = if self.is_shutting_down.get() {
            return Err(Error::ShuttingDown);
        } else {
            self.hinstance
        };

        let os_title = {
            use std::{ffi::OsStr, os::windows::prelude::OsStrExt};
            let mut buffer: Vec<u16> = OsStr::new(config.title).encode_wide().collect();
            buffer.push(0);
            buffer
        };

        // SAFETY: We need to increment the strong count because we are passing
        // the pointer to the OS. Under no circumstances must `Inner` be
        // permitted to drop while a window is active.
        unsafe { Rc::increment_strong_count(self) };

        // SAFETY: We intentionally insert a type here so that the compiler
        // warns us if the type of `shell.inner` changes for any reason.
        let raw_inner_ptr: InnerPtr = Rc::into_raw((*self).clone());

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
                raw_inner_ptr.cast(),
            )
        };

        self.windows.borrow_mut().push(hwnd);
        unsafe { ShowWindow(hwnd, SW_SHOW) };

        Ok(super::WindowId(WindowId { hwnd }))
    }

    fn destroy_window(&self, window: super::WindowId) {
        unsafe { PostMessageW(window.0.hwnd, UM_DESTROY_WINDOW, WPARAM(0), LPARAM(0)) };
    }

    fn show_window(&self, window: super::WindowId) {
        unsafe { ShowWindow(window.0.hwnd, SW_SHOW) };
    }

    fn hide_window(&self, window: super::WindowId) {
        unsafe { ShowWindow(window.0.hwnd, SW_HIDE) };
    }

    fn hwnd(&self, window: super::WindowId) -> windows::Win32::Foundation::HWND {
        window.0.hwnd
    }
}

#[inline]
fn dispatch(shell: &Rc<Inner>, events: impl IntoIterator<Item = Event>) {
    let mut cb = shell.event_callback.borrow_mut();

    // If we don't have a callback yet, the event was sent before
    // run_event_loop() was called. This happens when windows are created before
    // the event loop is run.
    if let Some(callback) = cb.as_mut() {
        let mut ctrl = EventLoopControl::Poll;
        for event in events {
            callback(event, shell, &mut ctrl);
            shell.event_mode.set(ctrl);

            if ctrl == EventLoopControl::Exit {
                // std::mem::drop(cb);
                // clean_exit(shell);
                unsafe { PostQuitMessage(0) };
            }
        }
    } else {
        shell.init_event_buffer.borrow_mut().extend(events);
    }
}

fn clean_exit(shell: &Rc<Inner>) -> ! {
    // Cleanly shut down the event loop by disabling window creation and
    // destroying any windows that remain.
    shell.is_shutting_down.set(true);

    let mut cb = shell.event_callback.borrow_mut();

    // If we don't have a callback yet, the event was sent before
    // run_event_loop() was called. This happens when windows are created before
    // the event loop is run.
    if let Some(callback) = cb.as_mut() {
        let mut ctrl = EventLoopControl::Poll;

        for window in shell.windows.borrow_mut().drain(..) {
            callback(
                Event::Window {
                    window_id: window.into(),
                    event: WindowEvent::Destroyed,
                },
                shell,
                &mut ctrl,
            );
        }
    } else {
        // NOTE(straivers): is this state possible?
    }

    unsafe { PostQuitMessage(0) };

    std::process::exit(0);
}

unsafe extern "system" fn unsafe_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if msg == WM_CREATE {
        let create_struct = lparam.0 as *const CREATESTRUCTW;
        let window = (*create_struct).lpCreateParams.cast::<InnerPtr>();
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, window as isize);
    }

    let shell = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as InnerPtr;
    if let Some(shell) = shell.as_ref() {
        match msg {
            WM_DESTROY => {
                // Decrement the reference count since the OS no longer owns this
                // reference to Inner.
                let shell = Rc::from_raw(shell);
                shell.windows.borrow_mut().retain(|h| *h != hwnd);
                wndproc(&shell, hwnd, msg, wparam, lparam);
                LRESULT(0)
            }
            UM_DESTROY_WINDOW => {
                DestroyWindow(hwnd);
                LRESULT(0)
            }
            rest => {
                // NOTE(straivers): there must be a way to avoid
                // from_raw/into_raw, surely?
                let shell = Rc::from_raw(shell);
                let r = wndproc(&shell, hwnd, rest, wparam, lparam);
                let _ = Rc::into_raw(shell);
                r
            }
        }
    } else {
        DefWindowProcW(hwnd, msg, wparam, lparam)
    }
}

#[allow(clippy::too_many_lines)]
fn wndproc(shell: &Rc<Inner>, hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let window_id = hwnd.into();

    let event = match msg {
        WM_CREATE => {
            let rect: RECT = {
                let mut rect = RECT::default();
                unsafe {
                    GetClientRect(hwnd, std::ptr::addr_of_mut!(rect));
                }
                rect
            };

            let extent = Extent {
                width: Px((rect.right - rect.left).try_into().unwrap()),
                height: Px((rect.bottom - rect.top).try_into().unwrap()),
            };

            Event::Window {
                window_id,
                event: WindowEvent::Init {
                    inner_extent: extent,
                },
            }
        }
        WM_DESTROY => Event::Window {
            window_id,
            event: WindowEvent::Destroyed,
        },
        WM_CLOSE => Event::Window {
            window_id,
            event: WindowEvent::CloseRequested,
        },
        WM_MOUSEMOVE => {
            let x = Px(lparam.0 as i16);
            let y = Px((lparam.0 >> 16) as i16);
            Event::Window {
                window_id,
                event: WindowEvent::CursorMoved {
                    position: Point { x, y },
                },
            }
        }
        WM_LBUTTONDOWN => Event::Window {
            window_id,
            event: WindowEvent::LeftMouseButtonPressed,
        },
        WM_LBUTTONUP => Event::Window {
            window_id,
            event: WindowEvent::LeftMouseButtonReleased,
        },
        WM_RBUTTONDOWN => Event::Window {
            window_id,
            event: WindowEvent::RightMouseButtonPressed,
        },
        WM_RBUTTONUP => Event::Window {
            window_id,
            event: WindowEvent::RightMouseButtonReleased,
        },
        WM_MBUTTONDOWN => Event::Window {
            window_id,
            event: WindowEvent::MiddleMouseButtonPressed,
        },
        WM_MBUTTONUP => Event::Window {
            window_id,
            event: WindowEvent::MiddleMouseButtonReleased,
        },
        special_return => {
            return match special_return {
                WM_ERASEBKGND => LRESULT(1),
                WM_WINDOWPOSCHANGING => {
                    let pos = lparam.0 as *mut WINDOWPOS;
                    // NOTE(straivers): Since we redraw the entire window
                    // anyway, there's no need to preserve the old framebuffer.
                    unsafe { (*pos).flags |= SWP_NOCOPYBITS };
                    LRESULT(0)
                }
                WM_WINDOWPOSCHANGED => {
                    let pos = lparam.0 as *const WINDOWPOS;

                    let (width, height) = unsafe { ((*pos).cx, (*pos).cy) };
                    let width = width as i16;
                    let height = height as i16;

                    let resize = Event::Window {
                        window_id,
                        event: WindowEvent::Resized {
                            inner_extent: Extent {
                                width: Px(width),
                                height: Px(height),
                            },
                        },
                    };

                    dispatch(
                        shell,
                        std::iter::once(resize).chain(
                            shell
                                .windows
                                .borrow()
                                .iter()
                                .map(|hwnd| Event::Window {
                                    window_id: (*hwnd).into(),
                                    event: WindowEvent::Repaint,
                                })
                                .chain(std::iter::once(Event::RepaintComplete)),
                        ),
                    );

                    LRESULT(0)
                }
                WM_PAINT => {
                    dispatch(
                        shell,
                        std::iter::once(Event::Window {
                            window_id,
                            event: WindowEvent::Repaint,
                        }),
                    );

                    unsafe {
                        let mut ps = PAINTSTRUCT::default();
                        BeginPaint(hwnd, &mut ps);
                        EndPaint(hwnd, &ps);
                    }

                    LRESULT(0)
                }
                _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
            };
        }
    };

    dispatch(shell, std::iter::once(event));

    LRESULT(0)
}
