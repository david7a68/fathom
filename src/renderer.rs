use std::{
    collections::{HashMap, HashSet},
    ffi::CStr,
    os::raw::c_char,
};

use ash::vk;
use windows::Win32::{
    Foundation::{HINSTANCE, HWND, RECT},
    UI::WindowsAndMessaging::GetClientRect,
};

const VALIDATION_LAYER: *const i8 = b"VK_LAYER_KHRONOS_validation\0".as_ptr().cast();

const INSTANCE_EXTENSIONS: [*const i8; 2] = [
    b"VK_KHR_surface\0".as_ptr().cast(),
    #[cfg(target_os = "windows")]
    b"VK_KHR_win32_surface\0".as_ptr().cast(),
];

const DEVICE_EXTENSIONS: [*const i8; 1] = [b"VK_KHR_swapchain\0".as_ptr().cast()];

const DESIRED_SWAPCHAIN_LENGTH: u32 = 2;

const UI_FRAG_SHADER_SPV: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/ui.frag.spv"));
const UI_VERT_SHADER_SPV: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/ui.vert.spv"));

pub struct Renderer {
    #[allow(dead_code)]
    entry: ash::Entry,
    instance: ash::Instance,

    surface_api: ash::extensions::khr::Surface,

    #[cfg(target_os = "windows")]
    os_surface_api: ash::extensions::khr::Win32Surface,

    device: Option<Device>,
    swapchains: HashMap<vk::SwapchainKHR, Swapchain>,
}

pub struct Device {
    device: ash::Device,
    physical_device: vk::PhysicalDevice,
    swapchain_api: ash::extensions::khr::Swapchain,

    graphics_family: u32,
    present_family: u32,
    graphics_queue: vk::Queue,
    present_queue: vk::Queue,
}

pub struct Swapchain {
    format: vk::Format,
    surface: vk::SurfaceKHR,
    images: Vec<vk::Image>,
    image_views: Vec<vk::ImageView>,
}

impl Renderer {
    pub fn new() -> Self {
        let entry = unsafe { ash::Entry::load().unwrap() };

        let instance = {
            let app_info =
                vk::ApplicationInfo::builder().api_version(vk::make_api_version(0, 1, 1, 0));

            let mut instance_layers = vec![];

            #[cfg(debug_assertions)]
            {
                let has_layers = has_required_names(
                    &entry.enumerate_instance_layer_properties().unwrap(),
                    |l| &l.layer_name,
                    &[VALIDATION_LAYER],
                );

                if has_layers[0] {
                    instance_layers.push(VALIDATION_LAYER);
                }
            }

            let extensions = INSTANCE_EXTENSIONS;

            {
                let has_required = has_required_names(
                    &entry.enumerate_instance_extension_properties(None).unwrap(),
                    |e| &e.extension_name,
                    &INSTANCE_EXTENSIONS,
                );

                for (index, result) in has_required.iter().enumerate() {
                    assert!(
                        result,
                        "required Vulkan extension not found: {:?}",
                        unsafe { CStr::from_ptr(extensions[index]) }
                    );
                }
            };

            let instance_ci = vk::InstanceCreateInfo::builder()
                .application_info(&app_info)
                .enabled_layer_names(&instance_layers)
                .enabled_extension_names(&extensions);

            unsafe { entry.create_instance(&instance_ci, None).unwrap() }
        };

        let surface_api = { ash::extensions::khr::Surface::new(&entry, &instance) };

        #[cfg(target_os = "windows")]
        let os_surface_api = { ash::extensions::khr::Win32Surface::new(&entry, &instance) };

        Self {
            entry,
            instance,
            surface_api,
            os_surface_api,
            device: None,
            swapchains: HashMap::new(),
        }
    }

    #[cfg(target_os = "windows")]
    pub fn create_swapchain(&mut self, hwnd: HWND, hinstance: HINSTANCE) -> vk::SwapchainKHR {
        let surface_ci = vk::Win32SurfaceCreateInfoKHR::builder()
            .hinstance(hinstance.0 as _)
            .hwnd(hwnd.0 as _);

        let surface = unsafe {
            self.os_surface_api
                .create_win32_surface(&surface_ci, None)
                .unwrap()
        };

        let extent = unsafe {
            let mut rect: RECT = std::mem::zeroed();
            GetClientRect(hwnd, &mut rect);
            vk::Extent2D {
                width: u32::try_from(rect.right).unwrap(),
                height: u32::try_from(rect.bottom).unwrap(),
            }
        };

        self.actually_create_swapchain(surface, extent)
    }

    pub fn destroy_swapchain(&mut self, swapchain: vk::SwapchainKHR) {
        if let Some(data) = self.swapchains.remove(&swapchain) {
            self.destroy_swapchain_data(swapchain, data);
        }
    }

    fn destroy_swapchain_data(&self, handle: vk::SwapchainKHR, mut data: Swapchain) {
        let device = self.device.as_ref().unwrap();
        unsafe {
            for view in data.image_views.drain(..) {
                device.device.destroy_image_view(view, None);
            }

            device.swapchain_api.destroy_swapchain(handle, None);
            self.surface_api.destroy_surface(data.surface, None);
        }
    }

    fn create_device(&self, surface: vk::SurfaceKHR) -> Device {
        let selected_device = {
            let mut selected_device = None;

            for physical_device in unsafe { self.instance.enumerate_physical_devices().unwrap() } {
                let mut found_present_family = false;
                let mut found_graphics_family = false;
                let mut present_family = 0;
                let mut graphics_family = 0;

                let queue_families = unsafe {
                    self.instance
                        .get_physical_device_queue_family_properties(physical_device)
                };

                for (index, queue_family) in queue_families.iter().rev().enumerate() {
                    let index = index.try_into().unwrap();

                    if unsafe {
                        self.surface_api
                            .get_physical_device_surface_support(physical_device, index, surface)
                            .unwrap()
                    } {
                        found_present_family = true;
                        present_family = index;
                    };

                    if queue_family.queue_flags.contains(vk::QueueFlags::GRAPHICS) {
                        found_graphics_family = true;
                        graphics_family = index;
                    }

                    if found_graphics_family && found_present_family {
                        break;
                    }
                }

                if !found_present_family || !found_graphics_family {
                    continue;
                }

                let supports_swapchain = has_required_names(
                    &unsafe {
                        self.instance
                            .enumerate_device_extension_properties(physical_device)
                            .unwrap()
                    },
                    |e| &e.extension_name,
                    &DEVICE_EXTENSIONS,
                )[0];

                if !supports_swapchain {
                    continue;
                }

                selected_device = Some((physical_device, graphics_family, present_family));
                break;
            }

            selected_device
        };

        if let Some((physical_device, present_family, graphics_family)) = selected_device {
            let queue_priority = 1.0;

            let queue_ci = [
                vk::DeviceQueueCreateInfo::builder()
                    .queue_family_index(graphics_family)
                    .queue_priorities(&[queue_priority])
                    .build(),
                vk::DeviceQueueCreateInfo::builder()
                    .queue_family_index(present_family)
                    .queue_priorities(&[queue_priority])
                    .build(),
            ];

            let num_queues = 1 + usize::from(present_family != graphics_family);
            let device_ci = vk::DeviceCreateInfo::builder()
                .queue_create_infos(&queue_ci[0..num_queues])
                .enabled_extension_names(&DEVICE_EXTENSIONS);

            let device = unsafe {
                self.instance
                    .create_device(physical_device, &device_ci, None)
                    .unwrap()
            };

            let graphics_queue = unsafe { device.get_device_queue(graphics_family, 0) };
            let present_queue = unsafe { device.get_device_queue(present_family, 0) };

            let swapchain_api = { ash::extensions::khr::Swapchain::new(&self.instance, &device) };

            Device {
                device,
                physical_device,
                swapchain_api,
                graphics_family,
                present_family,
                graphics_queue,
                present_queue,
            }
        } else {
            // TODO(straivers): explain why
            panic!("no viable Vulkan device found supporting both graphics and presentation")
        }
    }

    fn actually_create_swapchain(
        &mut self,
        surface: vk::SurfaceKHR,
        extent: vk::Extent2D,
    ) -> vk::SwapchainKHR {
        let device = self.device.get_or_insert(self.create_device(surface));

        let format = {
            let formats = unsafe {
                self.surface_api
                    .get_physical_device_surface_formats(device.physical_device, surface)
                    .unwrap()
            };

            assert!(!formats.is_empty());

            formats
                .iter()
                .find_map(|f| (f.format == vk::Format::B8G8R8A8_SRGB).then_some(*f))
                .unwrap_or(formats[0])
        };

        let (extent, transform, swapchain_length) = {
            let capabilities = unsafe {
                self.surface_api
                    .get_physical_device_surface_capabilities(device.physical_device, surface)
                    .unwrap()
            };

            let extent = if capabilities.current_extent.width == u32::MAX {
                vk::Extent2D {
                    width: extent.width.clamp(
                        capabilities.min_image_extent.width,
                        capabilities.max_image_extent.width,
                    ),
                    height: extent.height.clamp(
                        capabilities.min_image_extent.height,
                        capabilities.max_image_extent.height,
                    ),
                }
            } else {
                capabilities.current_extent
            };

            assert!(
                extent.width > 0 && extent.height > 0,
                "a swapchain must always have nonzero extent"
            );

            let length = if capabilities.max_image_count == 0
                || capabilities.min_image_count <= DESIRED_SWAPCHAIN_LENGTH
            {
                DESIRED_SWAPCHAIN_LENGTH
            } else {
                capabilities.min_image_count
            };

            (extent, capabilities.current_transform, length)
        };

        let concurrent_family_indices = &[device.graphics_family, device.present_family];
        let swapchain_ci = vk::SwapchainCreateInfoKHR::builder()
            .surface(surface)
            .min_image_count(swapchain_length)
            .image_format(format.format)
            .image_color_space(format.color_space)
            .image_extent(extent)
            .image_array_layers(1)
            .image_usage(vk::ImageUsageFlags::COLOR_ATTACHMENT)
            .queue_family_indices(concurrent_family_indices)
            .pre_transform(transform)
            .composite_alpha(vk::CompositeAlphaFlagsKHR::OPAQUE)
            .present_mode(vk::PresentModeKHR::FIFO)
            .clipped(true);

        let swapchain_ci = if device.graphics_family == device.present_family {
            swapchain_ci.image_sharing_mode(vk::SharingMode::EXCLUSIVE)
        } else {
            swapchain_ci.image_sharing_mode(vk::SharingMode::CONCURRENT)
        };

        let swapchain = unsafe {
            device
                .swapchain_api
                .create_swapchain(&swapchain_ci, None)
                .unwrap()
        };

        let images = unsafe {
            device
                .swapchain_api
                .get_swapchain_images(swapchain)
                .unwrap()
        };

        let image_views = {
            let mut views = Vec::with_capacity(images.len());
            for image in &images {
                let view_ci = vk::ImageViewCreateInfo::builder()
                    .image(*image)
                    .view_type(vk::ImageViewType::TYPE_2D)
                    .format(format.format)
                    .components(vk::ComponentMapping::default())
                    .subresource_range(vk::ImageSubresourceRange {
                        aspect_mask: vk::ImageAspectFlags::COLOR,
                        base_mip_level: 0,
                        level_count: 1,
                        base_array_layer: 0,
                        layer_count: 1,
                    });

                let view = unsafe { device.device.create_image_view(&view_ci, None).unwrap() };
                views.push(view);
            }
            views
        };

        self.swapchains.insert(
            swapchain,
            Swapchain {
                surface,
                format: format.format,
                images,
                image_views,
            },
        );
        swapchain
    }
}

impl Drop for Renderer {
    fn drop(&mut self) {
        if let Some(device) = self.device.take() {
            for (handle, data) in std::mem::take(&mut self.swapchains) {
                self.destroy_swapchain_data(handle, data);
            }

            unsafe {
                device.device.destroy_device(None);
            }
        }

        unsafe {
            self.instance.destroy_instance(None);
        }
    }
}

fn has_required_names<T, F: Fn(&T) -> &[c_char], const N: usize>(
    items: &[T],
    to_name: F,
    names: &[*const c_char; N],
) -> [bool; N] {
    let mut item_set = HashSet::new();

    for name in items.iter().map(to_name) {
        item_set.insert(unsafe { CStr::from_ptr(name.as_ptr()) });
    }

    let mut results = [false; N];
    for i in 0..names.len() {
        results[i] = item_set.contains(unsafe { CStr::from_ptr(names[i]) });
    }

    results
}
