//! Vulkan renderer backend.
//!
//! Creates a Vulkan instance, device, swapchain, and provides a minimal
//! rendering interface for displaying the PS4 framebuffer.

use ash::vk;
use std::ffi::CStr;
use std::sync::{Arc, Mutex};
use winit::window::Window;
use raw_window_handle::{HasDisplayHandle, HasWindowHandle};

/// Vulkan renderer state.
pub struct VulkanRenderer {
    pub entry: ash::Entry,
    pub instance: ash::Instance,
    pub physical_device: vk::PhysicalDevice,
    pub device: ash::Device,
    pub graphics_queue: vk::Queue,
    pub graphics_queue_family: u32,
    pub surface_loader: Option<ash::khr::surface::Instance>,
    pub surface: Option<vk::SurfaceKHR>,
    pub swapchain_loader: Option<ash::khr::swapchain::Device>,
    pub swapchain: Option<vk::SwapchainKHR>,
    pub swapchain_images: Vec<vk::Image>,
    pub swapchain_image_views: Vec<vk::ImageView>,
    pub swapchain_format: vk::Format,
    pub swapchain_extent: vk::Extent2D,
    pub command_pool: vk::CommandPool,
    pub command_buffers: Vec<vk::CommandBuffer>,
    pub render_pass: Option<vk::RenderPass>,
    pub framebuffers: Vec<vk::Framebuffer>,
    pub image_available_semaphore: vk::Semaphore,
    pub render_finished_semaphore: vk::Semaphore,
    pub in_flight_fence: vk::Fence,
    /// Frame counter
    pub frame_count: u64,
}

impl VulkanRenderer {
    /// Create a new Vulkan renderer attached to a window.
    pub fn new(window: Arc<Window>) -> Result<Self, String> {
        let entry = unsafe { ash::Entry::load().map_err(|e| format!("Failed to load Vulkan: {}", e))? };

        // Get window handles
        let display_handle = window.display_handle().map_err(|e| e.to_string())?.as_raw();
        let window_handle = window.window_handle().map_err(|e| e.to_string())?.as_raw();

        // Enumerate required surface extensions
        let extension_names = ash_window::enumerate_required_extensions(display_handle)
            .map_err(|e| format!("Failed to enumerate surface extensions: {}", e))?;

        // Instance
        let app_info = vk::ApplicationInfo::default()
            .application_name(c"AnotherPS4")
            .application_version(vk::make_api_version(0, 0, 1, 0))
            .engine_name(c"AnotherPS4 GPU")
            .engine_version(vk::make_api_version(0, 0, 1, 0))
            .api_version(vk::make_api_version(0, 1, 3, 0));

        let instance_info = vk::InstanceCreateInfo::default()
            .application_info(&app_info)
            .enabled_extension_names(extension_names);

        let instance = unsafe {
            entry.create_instance(&instance_info, None)
                .map_err(|e| format!("Failed to create Vulkan instance: {}", e))?
        };

        let surface = unsafe {
            ash_window::create_surface(&entry, &instance, display_handle, window_handle, None)
                .map_err(|e| format!("Failed to create window surface: {}", e))?
        };
        let surface_loader = ash::khr::surface::Instance::new(&entry, &instance);

        // Physical device
        let physical_devices = unsafe {
            instance.enumerate_physical_devices()
                .map_err(|e| format!("Failed to enumerate physical devices: {}", e))?
        };

        if physical_devices.is_empty() {
            return Err("No Vulkan-capable GPU found".into());
        }

        let physical_device = physical_devices[0];
        let props = unsafe { instance.get_physical_device_properties(physical_device) };
        let device_name = unsafe { CStr::from_ptr(props.device_name.as_ptr()) };
        tracing::info!("Vulkan GPU: {:?} (Vulkan {}.{}.{})",
            device_name,
            vk::api_version_major(props.api_version),
            vk::api_version_minor(props.api_version),
            vk::api_version_patch(props.api_version)
        );

        // Find graphics queue family
        let queue_families = unsafe {
            instance.get_physical_device_queue_family_properties(physical_device)
        };
        let graphics_queue_family = queue_families.iter()
            .position(|qf| qf.queue_flags.contains(vk::QueueFlags::GRAPHICS))
            .ok_or("No graphics queue family found")? as u32;

        // Device
        let queue_priority = [1.0f32];
        let queue_info = vk::DeviceQueueCreateInfo::default()
            .queue_family_index(graphics_queue_family)
            .queue_priorities(&queue_priority);

        let device_extensions = [ash::khr::swapchain::NAME.as_ptr()];

        let device_info = vk::DeviceCreateInfo::default()
            .queue_create_infos(std::slice::from_ref(&queue_info))
            .enabled_extension_names(&device_extensions);

        let device = unsafe {
            instance.create_device(physical_device, &device_info, None)
                .map_err(|e| format!("Failed to create Vulkan device: {}", e))?
        };

        let graphics_queue = unsafe { device.get_device_queue(graphics_queue_family, 0) };

        // Command pool
        let pool_info = vk::CommandPoolCreateInfo::default()
            .queue_family_index(graphics_queue_family)
            .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER);

        let command_pool = unsafe {
            device.create_command_pool(&pool_info, None)
                .map_err(|e| format!("Failed to create command pool: {}", e))?
        };

        // Sync objects
        let semaphore_info = vk::SemaphoreCreateInfo::default();
        let fence_info = vk::FenceCreateInfo::default()
            .flags(vk::FenceCreateFlags::SIGNALED);

        let image_available_semaphore = unsafe {
            device.create_semaphore(&semaphore_info, None).unwrap()
        };
        let render_finished_semaphore = unsafe {
            device.create_semaphore(&semaphore_info, None).unwrap()
        };
        let in_flight_fence = unsafe {
            device.create_fence(&fence_info, None).unwrap()
        };

        let swapchain_loader = ash::khr::swapchain::Device::new(&instance, &device);
        
        // Setup swapchain
        let surface_format = vk::SurfaceFormatKHR {
            format: vk::Format::B8G8R8A8_UNORM,
            color_space: vk::ColorSpaceKHR::SRGB_NONLINEAR,
        };
        let present_mode = vk::PresentModeKHR::FIFO;
        let swapchain_extent = vk::Extent2D { width: 1280, height: 720 };
        
        // Wait, need surface capabilities to check min_image_count
        let surface_caps = unsafe {
            surface_loader.get_physical_device_surface_capabilities(physical_device, surface).unwrap()
        };
        let mut image_count = surface_caps.min_image_count + 1;
        if surface_caps.max_image_count > 0 && image_count > surface_caps.max_image_count {
            image_count = surface_caps.max_image_count;
        }

        let swapchain_create_info = vk::SwapchainCreateInfoKHR::default()
            .surface(surface)
            .min_image_count(image_count)
            .image_color_space(surface_format.color_space)
            .image_format(surface_format.format)
            .image_extent(swapchain_extent)
            .image_usage(vk::ImageUsageFlags::COLOR_ATTACHMENT)
            .image_sharing_mode(vk::SharingMode::EXCLUSIVE)
            .pre_transform(surface_caps.current_transform)
            .composite_alpha(vk::CompositeAlphaFlagsKHR::OPAQUE)
            .present_mode(present_mode)
            .clipped(true)
            .image_array_layers(1);

        let swapchain = unsafe { swapchain_loader.create_swapchain(&swapchain_create_info, None).unwrap() };
        let swapchain_images = unsafe { swapchain_loader.get_swapchain_images(swapchain).unwrap() };

        let mut swapchain_image_views = Vec::with_capacity(swapchain_images.len());
        for &image in &swapchain_images {
            let create_view_info = vk::ImageViewCreateInfo::default()
                .view_type(vk::ImageViewType::TYPE_2D)
                .format(surface_format.format)
                .components(vk::ComponentMapping {
                    r: vk::ComponentSwizzle::R,
                    g: vk::ComponentSwizzle::G,
                    b: vk::ComponentSwizzle::B,
                    a: vk::ComponentSwizzle::A,
                })
                .subresource_range(vk::ImageSubresourceRange {
                    aspect_mask: vk::ImageAspectFlags::COLOR,
                    base_mip_level: 0,
                    level_count: 1,
                    base_array_layer: 0,
                    layer_count: 1,
                })
                .image(image);
            let image_view = unsafe { device.create_image_view(&create_view_info, None).unwrap() };
            swapchain_image_views.push(image_view);
        }

        tracing::info!("Vulkan renderer initialized (Windowed mode) Swapchain Size: {}x{}", swapchain_extent.width, swapchain_extent.height);

        Ok(VulkanRenderer {
            entry,
            instance,
            physical_device,
            device,
            graphics_queue,
            graphics_queue_family,
            surface_loader: Some(surface_loader),
            surface: Some(surface),
            swapchain_loader: Some(swapchain_loader),
            swapchain: Some(swapchain),
            swapchain_images,
            swapchain_image_views,
            swapchain_format: surface_format.format,
            swapchain_extent,
            command_pool,
            command_buffers: Vec::new(),
            render_pass: None,
            framebuffers: Vec::new(),
            image_available_semaphore,
            render_finished_semaphore,
            in_flight_fence,
            frame_count: 0,
        })
    }

    /// Present a clear-color frame to the active window surface
    pub fn present_blank_frame(&mut self, _r: f32, _g: f32, _b: f32) {
        if self.swapchain.is_none() || self.swapchain_loader.is_none() {
            return;
        }

        let swapchain_loader = self.swapchain_loader.as_ref().unwrap();
        let swapchain = self.swapchain.unwrap();

        unsafe {
            // Wait for previous frame's fence
            self.device.wait_for_fences(&[self.in_flight_fence], true, std::u64::MAX).unwrap();

            // Acquire next image
            let result = swapchain_loader.acquire_next_image(
                swapchain,
                std::u64::MAX,
                self.image_available_semaphore,
                vk::Fence::null(),
            );

            let image_index = match result {
                Ok((index, _)) => index,
                Err(vk::Result::ERROR_OUT_OF_DATE_KHR) | Err(vk::Result::SUBOPTIMAL_KHR) => {
                    // Out of date means we need to recreate the swapchain because the window was resized
                    // Since resizing isn't supported yet, we'll just ignore and drop the frame
                    return;
                }
                Err(e) => panic!("Failed to acquire swapchain image: {}", e),
            };

            self.device.reset_fences(&[self.in_flight_fence]).unwrap();

            // Dummy submit to signal finish
            let wait_semaphores = [self.image_available_semaphore];
            let signal_semaphores = [self.render_finished_semaphore];
            let wait_stages = [vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT];

            let submit_info = vk::SubmitInfo::default()
                .wait_semaphores(&wait_semaphores)
                .wait_dst_stage_mask(&wait_stages)
                .signal_semaphores(&signal_semaphores);

            self.device.queue_submit(self.graphics_queue, &[submit_info], self.in_flight_fence).unwrap();

            // Present the image to the Window Surface
            let swapchains = [swapchain];
            let image_indices = [image_index];
            let present_info = vk::PresentInfoKHR::default()
                .wait_semaphores(&signal_semaphores)
                .swapchains(&swapchains)
                .image_indices(&image_indices);

            let _ = swapchain_loader.queue_present(self.graphics_queue, &present_info);
        }

        self.frame_count += 1;
        if self.frame_count % 60 == 0 {
            tracing::debug!("Vulkan: {} frames rendered to surface", self.frame_count);
        }
    }

    /// Return the GPU name for logging.
    pub fn gpu_name(&self) -> String {
        let props = unsafe {
            self.instance.get_physical_device_properties(self.physical_device)
        };
        let name = unsafe { CStr::from_ptr(props.device_name.as_ptr()) };
        name.to_string_lossy().into_owned()
    }
}

impl Drop for VulkanRenderer {
    fn drop(&mut self) {
        unsafe {
            self.device.device_wait_idle().ok();
            self.device.destroy_fence(self.in_flight_fence, None);
            self.device.destroy_semaphore(self.render_finished_semaphore, None);
            self.device.destroy_semaphore(self.image_available_semaphore, None);
            for &fb in &self.framebuffers {
                self.device.destroy_framebuffer(fb, None);
            }
            if let Some(rp) = self.render_pass {
                self.device.destroy_render_pass(rp, None);
            }
            for &iv in &self.swapchain_image_views {
                self.device.destroy_image_view(iv, None);
            }
            if let (Some(loader), Some(sc)) = (&self.swapchain_loader, self.swapchain) {
                loader.destroy_swapchain(sc, None);
            }
            self.device.destroy_command_pool(self.command_pool, None);
            self.device.destroy_device(None);
            if let (Some(loader), Some(surface)) = (&self.surface_loader, self.surface) {
                loader.destroy_surface(surface, None);
            }
            self.instance.destroy_instance(None);
        }
        tracing::info!("Vulkan renderer destroyed");
    }
}
