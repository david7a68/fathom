use std::os::raw::c_char;
use std::{collections::HashSet, ffi::CStr};

use ash::vk;
use windows::Win32::Foundation::RECT;
use windows::Win32::UI::WindowsAndMessaging::GetClientRect;
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

const SWAPCHAIN_EXTENSION: &[u8] = b"VK_KHR_swapchain\0";

const DESIRED_SWAPCHAIN_LENGTH: u32 = 2;

const UI_FRAG_SHADER_SPV: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/ui.frag.spv"));
const UI_VERT_SHADER_SPV: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/ui.vert.spv"));

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
            api_version: vk::make_api_version(0, 1, 1, 0),
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
                    panic!("required Vulkan extension not found: {:?}", unsafe {
                        CStr::from_ptr(required[index].as_ptr())
                    });
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

    let (
        vk_device,
        vk_physical_device,
        _device_properties,
        _graphics_queue,
        graphics_family,
        _present_queue,
        present_family,
    ) = {
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

                let supports_swapchain = has_required_names(
                    &unsafe { vk_instance.enumerate_device_extension_properties(handle)? },
                    |e| &e.extension_name,
                    &[as_cchar_slice(SWAPCHAIN_EXTENSION)],
                )[0];

                if !supports_swapchain {
                    continue;
                }

                selected_device = Some((handle, properties, graphics_family, present_family));
                break;
            }

            selected_device
        };

        if let Some((handle, properties, present_family, graphics_family)) = selected_device {
            let queue_priority = 1.0;
            let queue_create_infos = {
                let mut queue_create_infos = [vk::DeviceQueueCreateInfo {
                    queue_family_index: graphics_family,
                    queue_count: 1,
                    p_queue_priorities: &queue_priority,
                    ..Default::default()
                }; 2];

                if graphics_family != present_family {
                    queue_create_infos[1].queue_family_index = present_family;
                }

                queue_create_infos
            };

            let device_ci = vk::DeviceCreateInfo {
                p_queue_create_infos: queue_create_infos.as_ptr(),
                queue_create_info_count: 1 + (graphics_family != present_family) as u32,
                enabled_extension_count: 1,
                pp_enabled_extension_names: [SWAPCHAIN_EXTENSION.as_ptr().cast()].as_ptr(),
                ..Default::default()
            };

            let device = unsafe { vk_instance.create_device(handle, &device_ci, None)? };
            let graphics_queue = unsafe { device.get_device_queue(graphics_family, 0) };
            let present_queue = unsafe { device.get_device_queue(present_family, 0) };

            (
                device,
                handle,
                properties,
                graphics_queue,
                graphics_family,
                present_queue,
                present_family,
            )
        } else {
            // TODO(straivers): explain why
            panic!("no viable Vulkan device found supporting both graphics and presentation")
        }
    };

    let vk_swapchain_api = { ash::extensions::khr::Swapchain::new(&vk_instance, &vk_device) };

    let (window_width, window_height) = unsafe {
        let mut rect: RECT = std::mem::zeroed();
        GetClientRect(hwnd, &mut rect);
        (
            u32::try_from(rect.right).unwrap(),
            u32::try_from(rect.bottom).unwrap(),
        )
    };

    let (vk_swapchain, vk_swapchain_format) = {
        let format = {
            let formats = unsafe {
                vk_surface_api
                    .get_physical_device_surface_formats(vk_physical_device, window_surface)?
            };

            assert!(!formats.is_empty());

            formats
                .iter()
                .find_map(|f| (f.format == vk::Format::B8G8R8A8_SRGB).then_some(*f))
                .unwrap_or(formats[0])
        };

        let present_mode = vk::PresentModeKHR::FIFO;

        let (extent, transform, swapchain_length) = {
            let capabilities = unsafe {
                vk_surface_api
                    .get_physical_device_surface_capabilities(vk_physical_device, window_surface)?
            };

            let extent = if capabilities.current_extent.width != u32::MAX {
                capabilities.current_extent
            } else {
                vk::Extent2D {
                    width: window_width.clamp(
                        capabilities.min_image_extent.width,
                        capabilities.max_image_extent.width,
                    ),
                    height: window_height.clamp(
                        capabilities.min_image_extent.height,
                        capabilities.max_image_extent.height,
                    ),
                }
            };

            assert!(extent.width > 0 && extent.height > 0);

            // NOTE(straivers): We want at least 3 images, but may need more if
            // the driver requires it. Using 1 image more than the driver
            // requires helps to minimize the chance that the driver will block
            // on internal operations. Ref:
            // https://vulkan-tutorial.com/Drawing_a_triangle/Presentation/Swap_chain
            // retrieved 2022/07/09

            let max_length = if capabilities.max_image_count == 0 {
                u32::MAX
            } else {
                capabilities.max_image_count
            };

            let length = (capabilities.min_image_count + 1)
                .max(DESIRED_SWAPCHAIN_LENGTH)
                .min(max_length);

            debug_assert!(length <= max_length);
            if max_length >= DESIRED_SWAPCHAIN_LENGTH {
                debug_assert!(length >= DESIRED_SWAPCHAIN_LENGTH);
            }

            (extent, capabilities.current_transform, length)
        };

        let mut swapchain_ci = vk::SwapchainCreateInfoKHR {
            surface: window_surface,
            min_image_count: swapchain_length,
            image_format: format.format,
            image_color_space: format.color_space,
            image_extent: extent,
            image_array_layers: 1,
            image_usage: vk::ImageUsageFlags::COLOR_ATTACHMENT,
            pre_transform: transform,
            composite_alpha: vk::CompositeAlphaFlagsKHR::OPAQUE,
            present_mode,
            clipped: vk::TRUE,
            old_swapchain: vk::SwapchainKHR::null(),
            ..Default::default()
        };

        if graphics_family == present_family {
            swapchain_ci.image_sharing_mode = vk::SharingMode::EXCLUSIVE;
        } else {
            swapchain_ci.image_sharing_mode = vk::SharingMode::CONCURRENT;
            swapchain_ci.queue_family_index_count = 2;
            swapchain_ci.p_queue_family_indices = [graphics_family, present_family].as_ptr();
        }

        (
            unsafe { vk_swapchain_api.create_swapchain(&swapchain_ci, None)? },
            format,
        )
    };

    let vk_swapchain_images = unsafe { vk_swapchain_api.get_swapchain_images(vk_swapchain)? };

    let vk_swapchain_image_views = {
        let mut views = Vec::with_capacity(vk_swapchain_images.len());

        for image in &vk_swapchain_images {
            let view_ci = vk::ImageViewCreateInfo {
                image: *image,
                view_type: vk::ImageViewType::TYPE_2D,
                format: vk_swapchain_format.format,
                components: vk::ComponentMapping {
                    r: vk::ComponentSwizzle::IDENTITY,
                    g: vk::ComponentSwizzle::IDENTITY,
                    b: vk::ComponentSwizzle::IDENTITY,
                    a: vk::ComponentSwizzle::IDENTITY,
                },
                subresource_range: vk::ImageSubresourceRange {
                    aspect_mask: vk::ImageAspectFlags::COLOR,
                    base_mip_level: 0,
                    level_count: 1,
                    base_array_layer: 0,
                    layer_count: 1,
                },
                ..Default::default()
            };

            let view = unsafe { vk_device.create_image_view(&view_ci, None)? };
            views.push(view);
        }

        views
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
        for view in vk_swapchain_image_views {
            vk_device.destroy_image_view(view, None);
        }

        vk_swapchain_api.destroy_swapchain(vk_swapchain, None);
        vk_surface_api.destroy_surface(window_surface, None);

        vk_device.destroy_device(None);
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

        let required = &[as_cchar_slice(b"three\0")];

        let result = has_required_names(available, |i| i, required);
        assert!(result[0]);
    }
}
