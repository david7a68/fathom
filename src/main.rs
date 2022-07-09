use std::collections::HashSet;
use std::os::raw::c_char;

use ash::vk;
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

const WINDOW_TITLE: &str = "Hello!";

const VALIDATION_LAYER: &[u8] = b"VK_LAYER_KHRONOS_validation\0";

const SURFACE_EXTENSION: &[u8] = b"VK_KHR_surface\0";
const OS_SURFACE_EXTENSION: &[u8] = b"VK_KHR_win32_surface\0";

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

    let vk_entry = unsafe { ash::Entry::load()? };

    let vk_instance = {
        let app_info = vk::ApplicationInfo {
            p_application_name: "Fathom".as_ptr().cast(),
            application_version: vk::make_api_version(0, 0, 1, 0),
            p_engine_name: "Fathom".as_ptr().cast(),
            engine_version: vk::make_api_version(0, 0, 1, 0),
            api_version: vk::make_api_version(0, 1, 0, 0),
            ..Default::default()
        };

        let mut instance_layers = vec![];

        #[cfg(debug_assertions)]
        {
            let has_layers = has_required_names(
                &vk_entry.enumerate_instance_layer_properties()?,
                |l| &l.layer_name,
                &[as_cchar_slice(VALIDATION_LAYER)],
            );

            if has_layers[0] {
                instance_layers.push(VALIDATION_LAYER.as_ptr().cast());
            }
        }

        let extensions = {
            let required = &[
                as_cchar_slice(SURFACE_EXTENSION),
                as_cchar_slice(OS_SURFACE_EXTENSION),
            ];
            let has_required = has_required_names(
                &vk_entry.enumerate_instance_extension_properties(None)?,
                |e| &e.extension_name,
                required,
            );

            for (index, result) in has_required.iter().enumerate() {
                if !result {
                    panic!("required Vulkan extension not found: {:?}", required[index]);
                }
            }

            &[
                SURFACE_EXTENSION.as_ptr().cast(),
                OS_SURFACE_EXTENSION.as_ptr().cast(),
            ]
        };

        let instance_ci = vk::InstanceCreateInfo {
            p_application_info: &app_info,
            enabled_layer_count: instance_layers.len() as u32,
            pp_enabled_layer_names: if instance_layers.is_empty() {
                std::ptr::null()
            } else {
                instance_layers.as_ptr()
            },
            enabled_extension_count: extensions.len() as u32,
            pp_enabled_extension_names: extensions.as_ptr(),
            ..Default::default()
        };

        unsafe { vk_entry.create_instance(&instance_ci, None)? }
    };

    let vk_surface_api = { ash::extensions::khr::Surface::new(&vk_entry, &vk_instance) };

    let vk_win32_surface_api = { ash::extensions::khr::Win32Surface::new(&vk_entry, &vk_instance) };

    let window_surface = {
        let surface_ci = vk::Win32SurfaceCreateInfoKHR {
            hinstance: hinstance.0 as _,
            hwnd: hwnd.0 as _,
            ..Default::default()
        };

        unsafe { vk_win32_surface_api.create_win32_surface(&surface_ci, None)? }
    };

    let (vk_device, _device_properties, _graphics_queue, _present_queue) = {
        let selected_device = {
            let mut selected_device = None;

            for handle in unsafe { vk_instance.enumerate_physical_devices()? } {
                let properties = unsafe { vk_instance.get_physical_device_properties(handle) };
                
                let mut present_family = None;
                let mut graphics_family = None;
                
                let queue_families =
                    unsafe { vk_instance.get_physical_device_queue_family_properties(handle) };
                for (index, queue_family) in queue_families.iter().enumerate() {
                    let index = index.try_into().unwrap();

                    let found_present_queue = unsafe {
                        vk_surface_api.get_physical_device_surface_support(
                            handle,
                            index,
                            window_surface,
                        )?
                    };

                    let found_graphics_queue =
                        queue_family.queue_flags.contains(vk::QueueFlags::GRAPHICS);

                    present_family = found_present_queue.then_some(index);
                    graphics_family = found_graphics_queue.then_some(index);

                    if present_family.is_some() && graphics_family.is_some() {
                        break;
                    }
                }

                let present_family = if let Some(present_family) = present_family {
                    present_family
                } else {
                    continue;
                };

                let graphics_family = if let Some(graphics_family) = graphics_family {
                    graphics_family
                } else {
                    continue;
                };

                selected_device = Some((handle, properties, present_family, graphics_family));
                break;
            }

            selected_device
        };

        if let Some((handle, properties, graphics_queue_family, present_queue_family)) =
            selected_device
        {
            let queue_priority = 1.0;
            let queue_create_infos = {
                let mut queue_create_infos = [vk::DeviceQueueCreateInfo {
                    queue_family_index: graphics_queue_family,
                    queue_count: 1,
                    p_queue_priorities: &queue_priority,
                    ..Default::default()
                }; 2];

                if graphics_queue_family != present_queue_family {
                    queue_create_infos[1].queue_family_index = present_queue_family;
                }

                queue_create_infos
            };

            let device_ci = vk::DeviceCreateInfo {
                p_queue_create_infos: queue_create_infos.as_ptr(),
                queue_create_info_count: 1 + (graphics_queue_family != present_queue_family) as u32,
                ..Default::default()
            };

            let device = unsafe { vk_instance.create_device(handle, &device_ci, None)? };
            let graphics_queue = unsafe { device.get_device_queue(graphics_queue_family, 0) };
            let present_queue = unsafe { device.get_device_queue(present_queue_family, 0) };

            (device, properties, graphics_queue, present_queue)
        } else {
            todo!("TODO: Handle no device found")
        }
    };

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

    unsafe {
        vk_device.destroy_device(None);
        vk_surface_api.destroy_surface(window_surface, None);
        vk_instance.destroy_instance(None);
    }

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

fn as_cchar_slice(original: &[u8]) -> &[i8] {
    unsafe { std::slice::from_raw_parts(original.as_ptr().cast(), original.len()) }
}

fn has_required_names<T, F: Fn(&T) -> &[c_char], const N: usize>(
    items: &[T],
    to_name: F,
    names: &[&[c_char]; N],
) -> [bool; N] {
    let mut item_set = HashSet::new();

    for name in items.iter().map(to_name) {
        // This is just a simple strnlen. should be fast enough for our purposes here
        let mut len = 0;
        while len < name.len() && name[len] != 0 {
            len += 1;
        }

        assert!(len < name.len());
        // +1 to account for the nul terminator
        item_set.insert(&name[0..len + 1]);
    }

    let mut results = [false; N];
    for i in 0..names.len() {
        results[i] = item_set.contains(names[i])
    }

    results
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn null_terminated_strings() {
        assert_eq!(VALIDATION_LAYER.last(), Some(&0));
        assert_eq!(SURFACE_EXTENSION.last(), Some(&0));
        assert_eq!(OS_SURFACE_EXTENSION.last(), Some(&0));
    }

    #[test]
    fn name_check() {
        let available = &[
            as_cchar_slice(b"one\0"),
            as_cchar_slice(b"two\0"),
            as_cchar_slice(b"three\0"),
        ];

        let required = &[
            as_cchar_slice(b"three\0")
        ];

        let result = has_required_names(available, |i| i, required);
        assert!(result[0]);
    }
}
