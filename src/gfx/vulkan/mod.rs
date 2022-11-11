mod api;
mod texture;
mod ui_shader;
mod window;

use std::{cell::RefCell, ffi::c_char};

use ash::vk;
use smallvec::SmallVec;

use crate::handle_pool::{Handle, HandlePool};

use self::{
    api::{MemoryUsage, Vulkan},
    texture::{Staging, Texture},
    window::Window,
};

use super::{
    geometry::{Extent, Rect},
    pixel_buffer::{PixelBuffer, PixelBufferView},
    DrawCommandList, Error, GfxDevice, ImageCopy, MAX_IMAGES, MAX_SWAPCHAINS,
};

const fn as_cchar_slice(slice: &[u8]) -> &[c_char] {
    unsafe { std::mem::transmute(slice) }
}

const VALIDATION_LAYER: &[c_char] = as_cchar_slice(b"VK_LAYER_KHRONOS_validation\0");

const REQUIRED_INSTANCE_LAYERS: &[&[c_char]] = &[];

const REQUIRED_INSTANCE_EXTENSIONS: &[&[c_char]] = &[
    as_cchar_slice(b"VK_KHR_surface\0"),
    #[cfg(target_os = "windows")]
    as_cchar_slice(b"VK_KHR_win32_surface\0"),
];

const OPTIONAL_INSTANCE_EXTENSIONS: &[&[c_char]] =
    &[as_cchar_slice(b"VK_EXT_swapchjain_colorspace\0")];

const REQUIRED_DEVICE_EXTENSIONS: &[&[c_char]] = &[as_cchar_slice(b"VK_KHR_swapchain\0")];

const OPTIONAL_DEVICE_EXTENSIONS: &[&[c_char]] = &[];

const FRAMES_IN_FLIGHT: usize = 2;
const PREFERRED_SWAPCHAIN_LENGTH: u32 = 2;

pub struct VulkanGfxDevice {
    api: Vulkan,

    windows: RefCell<HandlePool<Window, super::Swapchain, MAX_SWAPCHAINS>>,
    render_targets: RefCell<HandlePool<RenderTarget, super::RenderTarget, 128>>,
    images: RefCell<HandlePool<Texture, super::Image, MAX_IMAGES>>,
    staging: RefCell<Staging>,
}

impl VulkanGfxDevice {
    pub fn new(with_debug: bool) -> Result<Self, Error> {
        let mut optional_instance_layers = SmallVec::<[&[c_char]; 1]>::new();
        if with_debug {
            optional_instance_layers.push(VALIDATION_LAYER);
        }

        let api = Vulkan::new(
            REQUIRED_INSTANCE_LAYERS,
            &optional_instance_layers,
            REQUIRED_INSTANCE_EXTENSIONS,
            OPTIONAL_INSTANCE_EXTENSIONS,
            REQUIRED_DEVICE_EXTENSIONS,
            OPTIONAL_DEVICE_EXTENSIONS,
        )?;

        let staging = Staging::new(&api)?;

        Ok(Self {
            api,
            windows: RefCell::new(HandlePool::preallocate()),
            render_targets: RefCell::new(HandlePool::preallocate()),
            images: RefCell::new(HandlePool::preallocate_n(8)),
            staging: RefCell::new(staging),
        })
    }
}

impl Drop for VulkanGfxDevice {
    fn drop(&mut self) {
        self.staging.borrow_mut().destroy(&self.api);
    }
}

impl GfxDevice for VulkanGfxDevice {
    fn create_swapchain(
        &self,
        hwnd: windows::Win32::Foundation::HWND,
    ) -> Result<Handle<super::Swapchain>, Error> {
        let window = Window::new(&self.api, hwnd)?;
        Ok(self.windows.borrow_mut().insert(window)?)
    }

    fn resize_swapchain(
        &self,
        handle: Handle<super::Swapchain>,
        extent: Extent,
    ) -> Result<(), Error> {
        let mut windows = self.windows.borrow_mut();
        let window = windows.get_mut(handle)?;
        unsafe { self.api.device.device_wait_idle() }?;
        window.resize(&self.api, extent.into())?;
        Ok(())
    }

    fn destroy_swapchain(&self, handle: Handle<super::Swapchain>) -> Result<(), Error> {
        let mut windows = self.windows.borrow_mut();
        let window = windows.remove(handle)?;
        unsafe { self.api.device.device_wait_idle() }?;
        window.destroy(&self.api);
        Ok(())
    }

    fn get_next_swapchain_image(
        &self,
        handle: Handle<super::Swapchain>,
    ) -> Result<Handle<super::RenderTarget>, Error> {
        let mut windows = self.windows.borrow_mut();
        let window = windows.get_mut(handle)?;
        window.get_next_image(&self.api)?;

        let mut render_targets = self.render_targets.borrow_mut();
        let handle = render_targets.insert(RenderTarget::Swapchain(handle, window.frame_id()))?;
        Ok(handle)
    }

    fn present_swapchain_images(&self, handles: &[Handle<super::Swapchain>]) -> Result<(), Error> {
        let mut windows = self.windows.borrow_mut();
        for handle in handles {
            let window = windows.get_mut(*handle)?;
            match window.present(&self.api) {
                Ok(()) => Ok(()),
                Err(vk::Result::ERROR_OUT_OF_DATE_KHR) => Err(Error::SwapchainOutOfDate),
                Err(e) => Err(Error::VulkanInternal { error_code: e }),
            }?
        }
        Ok(())
    }

    fn create_image(&self, extent: Extent) -> Result<Handle<super::Image>, Error> {
        Ok(self
            .images
            .borrow_mut()
            .insert(Texture::new(&self.api, extent)?)?)
    }

    fn copy_pixels(
        &self,
        src: PixelBufferView,
        dst: Handle<super::Image>,
        ops: &[ImageCopy],
    ) -> Result<(), Error> {
        let mut images = self.images.borrow_mut();
        let image = images.get_mut(dst)?;
        self.staging
            .borrow_mut()
            .copy_pixels(&self.api, src, image, ops)?;
        Ok(())
    }

    fn copy_image(
        &self,
        src: Handle<super::Image>,
        dst: Handle<super::Image>,
        ops: &[ImageCopy],
    ) -> Result<(), Error> {
        todo!()
    }

    fn destroy_image(&self, handle: Handle<super::Image>) -> Result<(), Error> {
        let mut images = self.images.borrow_mut();
        // If is_idle() returns an error, remove the texture anyway.
        let texture = images.remove_if(handle, |t| t.is_idle(&self.api).unwrap_or(true))?;
        if let Some(texture) = texture {
            texture.destroy(&self.api);
            Ok(())
        } else {
            Err(Error::ResourceInUse)
        }
    }

    fn get_image_pixels(&self, handle: Handle<super::Image>) -> Result<PixelBuffer, Error> {
        todo!()
    }

    fn destroy_render_target(&self, handle: Handle<super::RenderTarget>) -> Result<(), Error> {
        let mut targets = self.render_targets.borrow_mut();
        let render_target = targets.remove(handle)?;

        match render_target {
            RenderTarget::Swapchain(..) => {
                // We don't need to do anything here. It doesn't matter if the
                // swapchain still exists or not, since we're not going to do
                // anything with it anymore.
            }
        }

        Ok(())
    }

    fn draw(
        &self,
        handle: Handle<super::RenderTarget>,
        commands: &DrawCommandList,
    ) -> Result<(), Error> {
        let mut targets = self.render_targets.borrow_mut();
        let render_target = targets.get(handle)?;

        match render_target {
            RenderTarget::Swapchain(window_handle, frame_id) => {
                let mut windows = self.windows.borrow_mut();

                // Check if the window still exists.
                if let Ok(window) = windows.get_mut(*window_handle) {
                    // Check if the render target is pointing to the current image
                    if *frame_id == window.frame_id() {
                        // The only way to get a swapchain image is to get a
                        // handle to it, and we've checked that the handle was
                        // acquired for the current frame.
                        let (frame, extent, sync, shader) = window.render_state();

                        frame.reset(&self.api)?;
                        frame
                            .geometry
                            .copy(&self.api, &commands.vertices, &commands.indices)?;

                        let used_textures = shader.apply(
                            &self.api,
                            commands,
                            frame.framebuffer,
                            extent,
                            &frame.geometry,
                            frame.command_buffer,
                        )?;

                        let mut wait_values = SmallVec::<[_; MAX_IMAGES as usize]>::new();
                        let mut wait_semaphores = SmallVec::<[_; MAX_IMAGES as usize]>::new();
                        let mut signal_values = SmallVec::<[_; MAX_IMAGES as usize]>::new();
                        let mut signal_semaphores = SmallVec::<[_; MAX_IMAGES as usize]>::new();

                        for texture in used_textures {
                            let mut images = self.images.borrow_mut();
                            let texture = images.get_mut(texture).unwrap();
                            texture.read_count += 1;

                            if let Some(write_state) = &texture.write_state {
                                if write_state.is_complete(&self.api)? {
                                    // Return the completed write to the staging
                                    // manager for reuse.
                                    self.staging
                                        .borrow_mut()
                                        .finish(texture.write_state.take().unwrap());
                                } else {
                                    // Make sure that the texture is not being written
                                    // to when we start using it (semaphore == count).
                                    wait_semaphores.push(write_state.semaphore);
                                    wait_values.push(write_state.counter);
                                }
                            }

                            // Increment the read semaphore when drawing is
                            // complete, and increment the check to match. Reads
                            // will be complete when semaphore == count.
                            signal_semaphores.push(texture.read_semaphore);
                            signal_values.push(texture.read_count);
                        }

                        let mut timeline_info = vk::TimelineSemaphoreSubmitInfo {
                            wait_semaphore_value_count: wait_semaphores.len() as u32,
                            p_wait_semaphore_values: wait_values.as_ptr(),
                            signal_semaphore_value_count: signal_semaphores.len() as u32,
                            p_signal_semaphore_values: signal_values.as_ptr(),
                            ..Default::default()
                        };

                        unsafe {
                            self.api.device.queue_submit(
                                self.api.graphics_queue,
                                &[vk::SubmitInfo::builder()
                                    .push_next(&mut timeline_info)
                                    .command_buffers(&[frame.command_buffer])
                                    .wait_semaphores(&[sync.acquire_semaphore])
                                    .signal_semaphores(&[sync.present_semaphore])
                                    .wait_dst_stage_mask(&[
                                        vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT,
                                    ])
                                    .build()],
                                frame.fence,
                            )
                        }?;

                        return Ok(());
                    }
                }

                targets.remove(handle).unwrap();
                // May be more accurate to have a RenderTargetOutOfDate or
                // ResourceOutOfDate error?
                Err(Error::InvalidHandle)
            }
        }
    }

    fn flush(&self) {
        unsafe { self.api.device.device_wait_idle() }.unwrap();
    }
}

/// Literally, a thing that can be rendered to.
enum RenderTarget {
    Swapchain(Handle<super::Swapchain>, u64),
}

/// Helper used to check if required and optional layers and extensions exist
/// within a set of items.
///
/// Returns `None` if one or more required names could not be found, or else
/// returns all the required names as well as every optional name that was
/// found.
fn has_names<T, F: Fn(&T) -> &[c_char]>(
    items: &[T],
    to_name: F,
    required: &[&[c_char]],
    optional: &[&[c_char]],
) -> Option<SmallVec<[*const c_char; 8]>> {
    let mut item_set = std::collections::HashSet::with_capacity(items.len());
    for item in items {
        let name = to_name(item);
        for (i, c) in name.iter().enumerate() {
            if *c == 0 {
                item_set.insert(&name[0..=i]);
                break;
            }
        }
    }

    let mut found_names = SmallVec::new();

    for name in required {
        if item_set.contains(name) {
            found_names.push(name.as_ptr());
        } else {
            return None;
        }
    }

    for name in optional {
        if item_set.contains(name) {
            found_names.push(name.as_ptr());
        }
    }

    Some(found_names)
}

impl From<crate::handle_pool::Error> for Error {
    fn from(e: crate::handle_pool::Error) -> Self {
        match e {
            crate::handle_pool::Error::InvalidHandle => Self::InvalidHandle,
            crate::handle_pool::Error::TooManyObjects {
                num_allocated: _,
                num_retired: _,
                capacity,
            }
            | crate::handle_pool::Error::Exhausted { capacity } => Self::TooManyObjects {
                limit: capacity as u32,
            },
        }
    }
}

impl From<Extent> for vk::Extent2D {
    fn from(e: Extent) -> Self {
        Self {
            width: e.width.0.try_into().unwrap(),
            height: e.height.0.try_into().unwrap(),
        }
    }
}

impl From<Rect> for vk::Rect2D {
    fn from(r: Rect) -> Self {
        Self {
            offset: vk::Offset2D {
                x: i32::from(r.left.0),
                y: i32::from(r.top.0),
            },
            extent: vk::Extent2D {
                width: (r.right - r.left).0 as u32,
                height: (r.bottom - r.top).0 as u32,
            },
        }
    }
}
