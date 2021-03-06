
use ash::vk;

use failure_derive::Fail;

use crate::context::instance::VkInstance;
use crate::context::device::{VkDevice, VkQueue};
use crate::context::surface::VkSurface;
use crate::ci::image::ImageViewCI;
use crate::ci::VkObjectBuildableCI;
use crate::error::{VkResult, VkError};
use crate::utils::time::VkTimeDuration;
use crate::{vkuint, vklint};

use std::ptr;

#[derive(Debug, Clone)]
pub struct SwapchainConfig {

    pub present_vsync: bool,
    pub image_acquire_time: VkTimeDuration,
}

impl Default for SwapchainConfig {

    fn default() -> SwapchainConfig {

        SwapchainConfig {
            present_vsync: false,
            image_acquire_time: VkTimeDuration::Infinite,
        }
    }
}

pub struct VkSwapchain {

    /// handle of `vk::SwapchainKHR`.
    handle: vk::SwapchainKHR,
    /// the extension loader provides functions for creation and destruction of `vk::SwapchainKHR` object.
    loader: ash::extensions::khr::Swapchain,

    /// the queue used to present image.
    present_queue: VkQueue,
    /// Image resources of current swapchain.
    pub images: Vec<SwapchainImage>,
    /// the format of presentable images.
    pub backend_format: vk::Format,
    /// the dimension of presentable images.
    pub dimension: vk::Extent2D,

    pub frame_in_flight: usize,

    image_acquire_time: vklint,

    config: SwapchainConfig,
}

pub struct SwapchainImage {

    /// the presentable image objects associated with the swapchain.
    ///
    /// These images are created in `loader.create_swapchain_khr(..)` call and are destroyed automatically when `vk::SwapchainKHR` is destroyed.
    pub image: vk::Image,
    /// the corresponding image views associated with the presentable images created by swapchain.
    pub view : vk::ImageView,
}

#[derive(Debug, Fail)]
pub enum SwapchainSyncError {
    #[fail(display = "No image became available within the time allowed.")]
    TimeOut,
    #[fail(display = "Swapchain does not match the surface properties exactly.")]
    SubOptimal,
    #[fail(display = "Surface has changed and is not compatible with the swapchain.")]
    SurfaceOutDate,
    #[fail(display = "Get unknown error when acquiring image.")]
    Unknown,
}

impl VkSwapchain {

    pub(crate) fn new(instance: &VkInstance, device: &VkDevice, surface: &VkSurface, config: SwapchainConfig, dimension: vk::Extent2D) -> VkResult<VkSwapchain> {

        VkSwapchain::build(instance, device, surface, config, dimension, None)
    }

    pub(crate) fn rebuild(&mut self, instance: &VkInstance, device: &VkDevice, surface: &VkSurface, dimension: vk::Extent2D) -> VkResult<()> {

        let new_chain = VkSwapchain::build(instance, device, surface, self.config.clone(), dimension, Some(self.handle))?;
        self.discard(device);
        *self = new_chain;

        Ok(())
    }

    fn build(instance: &VkInstance, device: &VkDevice, surface: &VkSurface, config: SwapchainConfig, dimension: vk::Extent2D, old_chain: Option<vk::SwapchainKHR>) -> VkResult<VkSwapchain> {

        let present_queue = query_present_queue(device, surface)
            .ok_or(VkError::custom("Graphics Queue is not support to present image to platform's surface."))?;
        let swapchain_format = query_optimal_format(device, surface)?;
        let swapchain_capability = query_swapchain_capability(device, surface, dimension)?;
        let swapchain_present_mode = query_optimal_present_mode(device, surface, &config)?;

        let swapchain_ci = vk::SwapchainCreateInfoKHR {
            s_type                   : vk::StructureType::SWAPCHAIN_CREATE_INFO_KHR,
            p_next                   : ptr::null(),
            flags                    : vk::SwapchainCreateFlagsKHR::empty(),
            surface                  : surface.handle,
            min_image_count          : swapchain_capability.desired_image_count,
            image_format             : swapchain_format.color_format,
            image_color_space        : swapchain_format.color_space,
            image_extent             : swapchain_capability.swapchain_extent,
            image_array_layers       : 1,
            image_usage              : swapchain_capability.support_usage,
            image_sharing_mode       : vk::SharingMode::EXCLUSIVE,
            queue_family_index_count : 0,
            p_queue_family_indices   : ptr::null(),
            pre_transform            : swapchain_capability.pre_transform,
            composite_alpha          : swapchain_capability.composite_alpha,
            present_mode             : swapchain_present_mode,
            // setting clipped to vk::TRUE allows the implementation to discard rendering outside of the surface area.
            clipped                  : vk::TRUE,
            old_swapchain: old_chain.and_then(|c| Some(c)).unwrap_or(vk::SwapchainKHR::null()),
        };

        let loader = ash::extensions::khr::Swapchain::new(&instance.handle, &device.logic.handle);

        let handle = unsafe {
            loader.create_swapchain(&swapchain_ci, None)
                .or(Err(VkError::create("Swapchain")))?
        };

        let image_resources = obtain_swapchain_images(device, handle, &loader, &swapchain_format)?;
        let frame_in_flight = image_resources.len();
        let image_acquire_time = config.image_acquire_time.into();

        let result = VkSwapchain {
            handle, loader, present_queue, frame_in_flight, image_acquire_time, config,
            images: image_resources,
            backend_format: swapchain_format.color_format,
            dimension: swapchain_capability.swapchain_extent,
        };

        Ok(result)
    }

    /// Acquire an available presentable image to use, and retrieve the index of that image.
    ///
    /// `sign_semaphore` is the semaphore to signal during this function, or None for no semaphore to signal.
    ///
    /// `sign_fence` is the fence to signal during this function, or None for no fence to signal.
    pub(crate) fn next_image(&self, semaphore: Option<vk::Semaphore>, fence: Option<vk::Fence>) -> Result<vkuint, SwapchainSyncError> {

        let semaphore = semaphore.unwrap_or(vk::Semaphore::null());
        let fence = fence.unwrap_or(vk::Fence::null());

        // execute next image acquire operation.
        let (image_index, is_sub_optimal) = unsafe {
            self.loader.acquire_next_image(self.handle, self.image_acquire_time, semaphore, fence)
                .map_err(|error| match error {
                    | vk::Result::TIMEOUT               => SwapchainSyncError::TimeOut,
                    | vk::Result::ERROR_OUT_OF_DATE_KHR => SwapchainSyncError::SurfaceOutDate,
                    | _ => SwapchainSyncError::Unknown,
                })?
        };

        if is_sub_optimal {
            Err(SwapchainSyncError::SubOptimal)
        } else {
            Ok(image_index)
        }
    }

    /// Queue an image for presentation.
    ///
    /// `wait_semaphores` specifies the semaphores to wait for before issuing the present request.
    ///
    /// `queue` is a queue that is capable of presentation to the target surface’s platform on the same device as the image’s swapchain.
    /// Generally it's a `vk::Queue` that is support `vk::QUEUE_GRAPHICS_BIT`.
    ///
    /// `image_index` is the index of swapchain’s presentable images.
    pub(crate) fn present(&self, wait_semaphores: &[vk::Semaphore], image_index: vkuint) -> Result<(), SwapchainSyncError> {

        // Currently only support single swapchain and single image index.
        let present_info = vk::PresentInfoKHR {
            s_type              : vk::StructureType::PRESENT_INFO_KHR,
            p_next              : ptr::null(),
            wait_semaphore_count: wait_semaphores.len() as _,
            p_wait_semaphores   : wait_semaphores.as_ptr(),
            swapchain_count     : 1,
            p_swapchains        : &self.handle,
            p_image_indices     : &image_index,
            p_results           : ptr::null_mut(),
        };

        let is_sub_optimal = unsafe {
            self.loader.queue_present(self.present_queue.handle, &present_info)
                .or(Err(SwapchainSyncError::Unknown))?
        };

        if is_sub_optimal {
            Err(SwapchainSyncError::SubOptimal)
        } else {
            Ok(())
        }
    }

    pub fn frame_in_flight(&self) -> usize {
        self.frame_in_flight.clone()
    }

    /// Destroy the `vk::SwapchainKHR` object.
    ///
    /// The application must not destroy `vk::SwapchainKHR` until after completion of all outstanding operations on images that were acquired from the `vk::SwapchainKHR`.
    pub(crate) fn discard(&self, device: &VkDevice) {

        self.images.iter().for_each(|swapchain_image| {
            device.discard(swapchain_image.view);
        });

        unsafe {
            self.loader.destroy_swapchain(self.handle, None);
        }
    }
}



// -----------------------------------------------------------------------------------
fn query_present_queue(device: &VkDevice, surface: &VkSurface) -> Option<VkQueue> {

    // TODO: Find an alternative queue if graphics queue is not support present operation.
    // just check if graphics queue support present operation.
    if surface.query_is_family_presentable(device.phy.handle, device.logic.queues.graphics.family_index) {
        Some(device.logic.queues.graphics.clone())
    } else {
        None
    }
}

fn obtain_swapchain_images(device: &VkDevice, swapchain: vk::SwapchainKHR, loader: &ash::extensions::khr::Swapchain, format: &SwapchainFormat) -> VkResult<Vec<SwapchainImage>> {

    let image_handles = unsafe {
        loader.get_swapchain_images(swapchain)
            .or(Err(VkError::query("Swapchain Images")))?
    };

    let mut result = Vec::with_capacity(image_handles.len());

    for image_handle in image_handles.into_iter() {

        let image_view = ImageViewCI::new(image_handle, vk::ImageViewType::TYPE_2D, format.color_format)
            .sub_range(vk::ImageSubresourceRange {
                aspect_mask: vk::ImageAspectFlags::COLOR,
                base_mip_level: 0,
                level_count: 1,
                base_array_layer: 0,
                layer_count: 1,
            })
            .build(device)?;

        let swapchain_image = SwapchainImage {
            image: image_handle,
            view : image_view,
        };
        result.push(swapchain_image);
    }

    Ok(result)
}
// -----------------------------------------------------------------------------------

// -----------------------------------------------------------------------------------
fn query_optimal_present_mode(device: &VkDevice, surface: &VkSurface, config: &SwapchainConfig) -> VkResult<vk::PresentModeKHR> {

    // select a present mode for the swapchain.
    let available_modes = surface.query_present_modes(device.phy.handle)?;

    // The vk::PresentModeKHR::FIFO mode must always be present as per spec.
    // This mode waits for the vertical blank ("v-sync").
    let result = if config.present_vsync {

        // if v-sync is not requested, try to find a mailbox mode.
        // it's the lowest latency non-tearing present mode available.
        let present_mode_searching = || {

            for present_mode in available_modes.into_iter() {
                if present_mode == vk::PresentModeKHR::MAILBOX {
                    return vk::PresentModeKHR::MAILBOX
                }

                if present_mode == vk::PresentModeKHR::IMMEDIATE {
                    return vk::PresentModeKHR::IMMEDIATE
                }
            }

            vk::PresentModeKHR::FIFO
        };

        present_mode_searching()
    } else {
        vk::PresentModeKHR::FIFO
    };

    Ok(result)
}
// -----------------------------------------------------------------------------------

// -----------------------------------------------------------------------------------
struct SwapchainFormat {
    color_format: vk::Format,
    color_space : vk::ColorSpaceKHR,
}

fn query_optimal_format(device: &VkDevice, surface: &VkSurface) -> VkResult<SwapchainFormat> {

    // Get list of supported surface formats.
    let support_formats = surface.query_formats(device.phy.handle)?;

    // If the surface format list only includes one entry with VK_FORMAT_UNDEFINED,
    // there is no preferred format, so we assume VK_FORMAT_B8G8R8A8_UNORM.
    let result = if support_formats.len() == 1 && support_formats[0].format == vk::Format::UNDEFINED {
        SwapchainFormat {
            color_format: vk::Format::B8G8R8A8_UNORM,
            color_space : support_formats[0].color_space,
        }
    } else {

        // iterate over the list of available surface format and check for the presence of VK_FORMAT_B8G8R8A8_UNORM.
        let format_searching = || {

            for surface_format in support_formats.iter() {

                if surface_format.format == vk::Format::B8G8R8A8_UNORM {
                    return SwapchainFormat {
                        color_format: surface_format.format,
                        color_space : surface_format.color_space,
                    }
                }
            }

            // in case VK_FORMAT_B8G8R8A8_UNORM is not available, select the first available color format.
            SwapchainFormat {
                color_format: support_formats[0].format,
                color_space : support_formats[0].color_space,
            }
        };

        format_searching()
    };

    Ok(result)
}
// -----------------------------------------------------------------------------------

// -----------------------------------------------------------------------------------
struct SwapchainCapability {

    support_usage: vk::ImageUsageFlags,
    desired_image_count: vkuint,
    swapchain_extent: vk::Extent2D,
    pre_transform: vk::SurfaceTransformFlagsKHR,
    composite_alpha: vk::CompositeAlphaFlagsKHR,
}

fn query_swapchain_capability(device: &VkDevice, surface: &VkSurface, dimension: vk::Extent2D) -> VkResult<SwapchainCapability> {

    let surface_caps = surface.query_capabilities(device.phy.handle)?;

    // Determine the usage of swapchain images. ---------------------
    let mut image_usage = vk::ImageUsageFlags::COLOR_ATTACHMENT;
    // Enable transfer source on swap chain images if supported
    if surface_caps.supported_usage_flags.contains(vk::ImageUsageFlags::TRANSFER_SRC) {
        image_usage |= vk::ImageUsageFlags::TRANSFER_SRC;
    }
    // Enable transfer destination on swap chain images if supported
    if surface_caps.supported_usage_flags.contains(vk::ImageUsageFlags::TRANSFER_DST) {
        image_usage |= vk::ImageUsageFlags::TRANSFER_DST;
    }
    // --------------------------------------------------------------

    // Determine the dimension of swapchain images. ------------------
    // If width (and height) equals the special value 0xFFFFFFFF, the size of the surface will be set by the swapchain.
    const SPECIAL_EXTEND: vkuint = 0xFFFF_FFFF;
    let optimal_extent = if surface_caps.current_extent.width == SPECIAL_EXTEND && surface_caps.current_extent.height == SPECIAL_EXTEND {
        // If the surface size is undefined, the size is set to the size of the images requested.
        use std::cmp::{max, min};

        vk::Extent2D {
            width: min(max(dimension.width, surface_caps.min_image_extent.width), surface_caps.max_image_extent.width),
            height: min(max(dimension.height, surface_caps.min_image_extent.height), surface_caps.max_image_extent.height),
        }
    } else {
        // If the surface size is defined, the swap chain size must match.
        surface_caps.current_extent.clone()
    };
    // --------------------------------------------------------------

    // Determine the number of images. ------------------------------
    let mut optimal_image_count = surface_caps.min_image_count + 1;
    if surface_caps.max_image_count > 0 && optimal_image_count > surface_caps.max_image_count {
        optimal_image_count = surface_caps.max_image_count;
    }
    // --------------------------------------------------------------

    // Find the transformation of the surface -----------------------
    let surface_transform = if surface_caps.supported_transforms.contains(vk::SurfaceTransformFlagsKHR::IDENTITY) {
        // We prefer a non-rotated transform.
        vk::SurfaceTransformFlagsKHR::IDENTITY
    } else {
        surface_caps.current_transform
    };
    // --------------------------------------------------------------

    // Find a supported composite alpha format (not all devices support alpha opaque).
    const CANDIDATE_COMPOSITE_ALPHAS: [vk::CompositeAlphaFlagsKHR; 4] = [
        vk::CompositeAlphaFlagsKHR::OPAQUE,
        vk::CompositeAlphaFlagsKHR::PRE_MULTIPLIED,
        vk::CompositeAlphaFlagsKHR::POST_MULTIPLIED,
        vk::CompositeAlphaFlagsKHR::INHERIT,
    ];

    // Simply select the first composite alpha format available.
    let composite_alpha_flag = CANDIDATE_COMPOSITE_ALPHAS.iter().find(|&&composite_alpha_flag| {
        surface_caps.supported_composite_alpha.contains(composite_alpha_flag)
    }).cloned().unwrap_or(vk::CompositeAlphaFlagsKHR::OPAQUE);
    // --------------------------------------------------------------

    let result = SwapchainCapability {
        support_usage: image_usage,
        desired_image_count: optimal_image_count,
        swapchain_extent: optimal_extent,
        pre_transform: surface_transform,
        composite_alpha: composite_alpha_flag,
    };
    Ok(result)
}
// -----------------------------------------------------------------------------------
