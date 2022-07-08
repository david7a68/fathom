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

const VULKAN_VALIDATION_LAYER_NAME: &str = "VK_LAYER_KHRONOS_validation\0";

const VULKAN_SURFACE_EXTENSION_NAME: &str = "VK_KHR_surface\0";
const VULKAN_WIN32_SURFACE_EXTENSION_NAME: &str = "VK_KHR_win32_surface\0";

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
        let os_title = wide_title(WINDOW_TITLE);

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
            let available_layers = vk_entry.enumerate_instance_layer_properties()?;

            for property in &available_layers {
                let name = {
                    let mut length = 0;
                    while length < property.layer_name.len() && property.layer_name[length] != 0 {
                        length += 1;
                    }

                    length += 1; // add the nul byte, since all layer names are nul-terminated
                    unsafe {
                        std::slice::from_raw_parts(
                            property.layer_name.as_ptr().cast::<u8>(),
                            length,
                        )
                    }
                };

                if name == VULKAN_VALIDATION_LAYER_NAME.as_bytes() {
                    instance_layers.push(VULKAN_VALIDATION_LAYER_NAME.as_ptr().cast());
                }
            }
        }

        let extensions = [
            VULKAN_SURFACE_EXTENSION_NAME.as_ptr().cast(),
            VULKAN_WIN32_SURFACE_EXTENSION_NAME.as_ptr().cast(),
        ];

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

            let available_devices = unsafe { vk_instance.enumerate_physical_devices()? };

            for handle in available_devices {
                let properties = unsafe { vk_instance.get_physical_device_properties(handle) };
                let queue_families =
                    unsafe { vk_instance.get_physical_device_queue_family_properties(handle) };

                let mut present_queue_family = None;
                let mut graphics_queue_family = None;

                for (index, queue_family) in queue_families.iter().enumerate() {
                    let index = index.try_into().unwrap();

                    let found_present_queue = unsafe {
                        vk_surface_api.get_physical_device_surface_support(
                            handle,
                            index,
                            window_surface,
                        )?
                    };

                    let found_graphics_queue = queue_family.queue_flags.contains(vk::QueueFlags::GRAPHICS);

                    present_queue_family = found_present_queue.then_some(index);
                    graphics_queue_family = found_graphics_queue.then_some(index);

                    if present_queue_family.is_some() && graphics_queue_family.is_some() {
                        break;
                    }
                }

                // TODO(straivers): check that the device supports the swapchain extension

                if let (Some(present_queue_family), Some(graphics_queue_family)) =
                    (present_queue_family, graphics_queue_family)
                {
                    selected_device = Some((
                        handle,
                        properties,
                        present_queue_family,
                        graphics_queue_family,
                    ));
                    break;
                }
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

            // TODO (straivers): Add swapchain extension
            let device_ci = vk::DeviceCreateInfo {
                p_queue_create_infos: queue_create_infos.as_ptr(),
                queue_create_info_count: queue_create_infos.len() as _,
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

/// Converts a string into a `Vec<u16>` of wide characters.
/// 
/// Note (straivers): For the moment, this allocates memory on the heap.
/// However, if things go as I expect once multiple windows are supported with
/// their associated per-frame bump allocators, these can be allocated from
/// there instead.
fn wide_title(title: &str) -> Vec<u16> {
    use std::{ffi::OsStr, os::windows::prelude::OsStrExt};

    let mut buffer: Vec<u16> = OsStr::new(title).encode_wide().collect();
    buffer.push(0);

    buffer
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wstr_conversion() {
        use std::{ffi::OsStr, os::windows::prelude::OsStrExt};

        let s = "Too many bagels tonight";
        let wstr = wide_title(s);
        let os_str: Vec<u16> = OsStr::new(s).encode_wide().collect();

        // Check that the conversion is correct, accounting for the null
        // terminator in `to_wstr()`.
        assert_eq!(&wstr.as_slice()[0..wstr.len() - 1], &os_str[..]);
    }

    #[test]
    fn null_terminated_strings() {
        assert_eq!(VULKAN_VALIDATION_LAYER_NAME.as_bytes().last(), Some(&0));
        assert_eq!(VULKAN_SURFACE_EXTENSION_NAME.as_bytes().last(), Some(&0));
        assert_eq!(VULKAN_WIN32_SURFACE_EXTENSION_NAME.as_bytes().last(), Some(&0));   
    }
}
