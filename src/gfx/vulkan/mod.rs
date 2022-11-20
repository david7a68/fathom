mod api;
mod texture;
mod ui_shader;
mod window;

use std::{cell::RefCell, ffi::c_char};

use arrayvec::ArrayVec;
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

const MAX_TEXTURE_DESCRIPTORS: u32 = MAX_IMAGES * 8;

pub struct VulkanGfxDevice {
    api: Vulkan,

    sampler: vk::Sampler,
    descriptor_sets: RefCell<ArrayVec<vk::DescriptorSet, { MAX_TEXTURE_DESCRIPTORS as usize }>>,
    descriptor_pool: vk::DescriptorPool,
    descriptor_layout: vk::DescriptorSetLayout,

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

        let sampler = {
            let create_info = vk::SamplerCreateInfo {
                mag_filter: vk::Filter::LINEAR,
                min_filter: vk::Filter::LINEAR,
                mipmap_mode: vk::SamplerMipmapMode::NEAREST,
                address_mode_u: vk::SamplerAddressMode::CLAMP_TO_BORDER,
                address_mode_v: vk::SamplerAddressMode::CLAMP_TO_BORDER,
                address_mode_w: vk::SamplerAddressMode::CLAMP_TO_BORDER,
                mip_lod_bias: 0.0,
                anisotropy_enable: vk::FALSE,
                max_anisotropy: 0.0,
                compare_enable: vk::FALSE,
                compare_op: vk::CompareOp::NEVER,
                min_lod: 0.0,
                max_lod: 0.0,
                border_color: vk::BorderColor::INT_OPAQUE_BLACK,
                unnormalized_coordinates: vk::TRUE,
                ..Default::default()
            };

            unsafe { api.device.create_sampler(&create_info, None) }?
        };

        let descriptor_layout = {
            let bindings = [vk::DescriptorSetLayoutBinding {
                binding: 0,
                descriptor_type: vk::DescriptorType::COMBINED_IMAGE_SAMPLER,
                descriptor_count: 1,
                stage_flags: vk::ShaderStageFlags::FRAGMENT,
                ..Default::default()
            }];

            let create_info = vk::DescriptorSetLayoutCreateInfo {
                binding_count: bindings.len() as u32,
                p_bindings: bindings.as_ptr(),
                ..Default::default()
            };

            unsafe { api.device.create_descriptor_set_layout(&create_info, None) }?
        };

        let descriptor_pool = {
            let pool_size = [vk::DescriptorPoolSize {
                ty: vk::DescriptorType::COMBINED_IMAGE_SAMPLER,
                descriptor_count: MAX_TEXTURE_DESCRIPTORS,
            }];

            let create_info = vk::DescriptorPoolCreateInfo {
                flags: vk::DescriptorPoolCreateFlags::UPDATE_AFTER_BIND,
                max_sets: MAX_TEXTURE_DESCRIPTORS,
                pool_size_count: pool_size.len() as u32,
                p_pool_sizes: pool_size.as_ptr(),
                ..Default::default()
            };

            unsafe { api.device.create_descriptor_pool(&create_info, None) }?
        };

        let descriptor_sets = {
            let layouts = [descriptor_layout; MAX_TEXTURE_DESCRIPTORS as usize];
            let create_info = vk::DescriptorSetAllocateInfo {
                descriptor_pool,
                descriptor_set_count: MAX_TEXTURE_DESCRIPTORS,
                p_set_layouts: layouts.as_ptr(),
                ..Default::default()
            };

            let mut sets = ArrayVec::new();
            unsafe {
                (api.device.fp_v1_0().allocate_descriptor_sets)(
                    api.device.handle(),
                    &create_info,
                    sets.as_mut_ptr(),
                )
                .result()?;
                sets.set_len(MAX_TEXTURE_DESCRIPTORS as usize);
            }
            sets
        };

        let staging = Staging::new(&api)?;

        Ok(Self {
            api,
            sampler,
            descriptor_sets: RefCell::new(descriptor_sets),
            descriptor_pool,
            descriptor_layout,
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
        let window = Window::new(&self.api, hwnd, self.descriptor_layout)?;
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
            }?;
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
        _src: Handle<super::Image>,
        _dst: Handle<super::Image>,
        _ops: &[ImageCopy],
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

    fn get_image_pixels(&self, _handle: Handle<super::Image>) -> Result<PixelBuffer, Error> {
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
        let mut wait_values = SmallVec::<[_; MAX_IMAGES as usize]>::new();
        let mut wait_semaphores = SmallVec::<[_; MAX_IMAGES as usize]>::new();
        let mut signal_values = SmallVec::<[_; MAX_IMAGES as usize]>::new();
        let mut signal_semaphores = SmallVec::<[_; MAX_IMAGES as usize]>::new();

        let mut targets = self.render_targets.borrow_mut();
        let mut windows = self.windows.borrow_mut();
        let (frame, extent, shader) = match targets.get(handle)? {
            RenderTarget::Swapchain(window_handle, frame_id) => {
                if let Ok(window) = windows.get_mut(*window_handle) {
                    if *frame_id == window.frame_id() {
                        let (frame, extent, sync, shader) = window.render_state();

                        // Vulkan Spec requires that wait_values.len() == wait_semaphores.len().
                        wait_values.push(0);
                        wait_semaphores.push(sync.acquire_semaphore);
                        // ditto
                        signal_values.push(0);
                        signal_semaphores.push(sync.present_semaphore);

                        (frame, extent, shader)
                    } else {
                        targets.remove(handle).unwrap();
                        Err(Error::InvalidHandle)?
                    }
                } else {
                    targets.remove(handle).unwrap();
                    // May be more accurate to have a RenderTargetOutOfDate or
                    // ResourceOutOfDate error?
                    Err(Error::InvalidHandle)?
                }
            }
        };

        frame.reset(&self.api)?;
        self.descriptor_sets
            .borrow_mut()
            .extend(frame.descriptors.drain(..));
        frame
            .geometry
            .copy(&self.api, &commands.vertices, &commands.indices)?;

        unsafe {
            self.api.device.begin_command_buffer(
                frame.command_buffer,
                &vk::CommandBufferBeginInfo::builder()
                    .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT),
            )
        }?;

        shader.begin(&self.api, frame.framebuffer, extent, frame.command_buffer);

        let mut used_textures = SmallVec::<[Handle<super::Image>; 32]>::new();
        for command in commands.commands.iter().chain(commands.current.as_ref()) {
            match command {
                super::Command::Scissor { rect } => unsafe {
                    self.api.device.cmd_set_scissor(
                        frame.command_buffer,
                        0,
                        &[vk::Rect2D::from(*rect)],
                    )
                },
                super::Command::Polygon {
                    first_index,
                    num_indices,
                } => shader.draw_indexed(
                    &self.api,
                    *first_index,
                    *num_indices,
                    extent,
                    &frame.geometry,
                    frame.command_buffer,
                ),
                super::Command::Image {
                    image,
                    first_index,
                    num_indices,
                } => {
                    let textures = self.images.borrow_mut();
                    // todo: cleanup if fails
                    let texture = textures.get(*image).unwrap();

                    debug_assert_eq!(texture.image_layout, vk::ImageLayout::READ_ONLY_OPTIMAL);
                    let texture_info = vk::DescriptorImageInfo {
                        sampler: self.sampler,
                        image_view: texture.image_view,
                        image_layout: vk::ImageLayout::READ_ONLY_OPTIMAL,
                    };

                    let descriptor = self.descriptor_sets.borrow_mut().pop().unwrap();
                    frame.descriptors.push(descriptor);

                    shader.draw_textured(
                        &self.api,
                        *first_index,
                        *num_indices,
                        extent,
                        &texture_info,
                        descriptor,
                        &frame.geometry,
                        frame.command_buffer,
                    );

                    used_textures.push(*image);
                    todo!()
                }
            }
        }

        shader.end(&self.api, frame.command_buffer);
        // todo cleanup on error
        unsafe { self.api.device.end_command_buffer(frame.command_buffer) }.unwrap();

        used_textures.sort();
        used_textures.dedup();
        for texture in used_textures {
            let mut images = self.images.borrow_mut();
            let texture = images.get_mut(texture).unwrap();
            texture.read_count += 1;

            if let Some(write_state) = &texture.write_state {
                // todo: what to do on failure
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
                    .wait_semaphores(&wait_semaphores)
                    .signal_semaphores(&signal_semaphores)
                    .wait_dst_stage_mask(&[vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT])
                    .build()],
                frame.fence,
            )
        }?;

        Ok(())
    }

    fn flush(&self) {
        unsafe { self.api.device.device_wait_idle() }.unwrap();
    }
}

/// Literally, a thing that can be rendered to.
enum RenderTarget {
    Swapchain(Handle<super::Swapchain>, u64),
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
