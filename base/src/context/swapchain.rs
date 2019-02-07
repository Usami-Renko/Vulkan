
use ash::vk;
use ash::version::DeviceV1_0;

use crate::context::instance::VkInstance;
use crate::context::device::VkDevice;
use crate::context::surface::VkSurface;
use crate::error::{VkResult, VkError};
use crate::vkuint;

use std::ptr;

pub struct SwapchainConfig {

    present_vsync: bool,
    dimension: vk::Extent2D,
}

pub struct VkSwapchain {

    /// handle of `vk::SwapchainKHR`.
    handle: vk::SwapchainKHR,
    /// the extension loader provides functions for creation and destruction of `vk::SwapchainKHR` object.
    loader: ash::extensions::khr::Swapchain,
    /// Image resources of current swapchain.
    images: Vec<SwapchainImage>,
    /// the format of presentable images.
    format: vk::Format,
    /// the dimension of presentable images.
    dimension: vk::Extent2D,
}

struct SwapchainImage {

    /// the presentable image objects associated with the swapchain.
    ///
    /// These images are created in `loader.create_swapchain_khr(..)` call and are destroyed automatically when `vk::SwapchainKHR` is destroyed.
    image: vk::Image,
    /// the corresponding image views associated with the presentable images created by swapchain.
    view : vk::ImageView,
}

impl VkSwapchain {

    pub fn new(instance: &VkInstance, device: &VkDevice, surface: &VkSurface, config: SwapchainConfig, old_chain: Option<VkSwapchain>) -> VkResult<VkSwapchain> {

        let swapchain_format = query_optimal_format(device, surface)?;
        let swapchain_capability = query_swapchain_capability(device, surface, &config)?;
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
            old_swapchain: old_chain.as_ref().and_then(|c| Some(c.handle)).unwrap_or(vk::SwapchainKHR::null()),
        };

        let loader = ash::extensions::khr::Swapchain::new(&instance.handle, &device.logic.handle);

        let handle = unsafe {
            loader.create_swapchain(&swapchain_ci, None)
                .or(Err(VkError::create("Swapchain")))?
        };

        // if an existing swap chain is re-created, destroy the old swap chain.
        // this also cleans up all the presentable images.
        if let Some(old_chain) = old_chain {
            old_chain.discard(device);
        }

        let image_resouces = obtain_swapchain_images(device, handle, &loader, &swapchain_format)?;
        let result = VkSwapchain {
            handle, loader,
            images: image_resouces,
            format: swapchain_format.color_format,
            dimension: swapchain_capability.swapchain_extent,
        };

        Ok(result)
    }

    fn discard(&self, device: &VkDevice) {

        unsafe {

            self.images.iter().for_each(|swapchain_image| {
                device.logic.handle.destroy_image_view(swapchain_image.view, None);
            });

            self.loader.destroy_swapchain(self.handle, None);
        }
    }
}


fn obtain_swapchain_images(device: &VkDevice, swapchain: vk::SwapchainKHR, loader: &ash::extensions::khr::Swapchain, format: &SwapchainFormat) -> VkResult<Vec<SwapchainImage>> {

    let image_handles = unsafe {
        loader.get_swapchain_images(swapchain)
            .or(Err(VkError::query("Swapchain Images")))?
    };

    let mut result = Vec::with_capacity(image_handles.len());

    for image_handle in image_handles.into_iter() {

        let view_ci = vk::ImageViewCreateInfo {
            s_type     : vk::StructureType::IMAGE_VIEW_CREATE_INFO,
            p_next     : ptr::null(),
            flags      : vk::ImageViewCreateFlags::empty(),
            image      : image_handle,
            view_type  : vk::ImageViewType::TYPE_2D,
            format     : format.color_format,
            components : vk::ComponentMapping {
                r: vk::ComponentSwizzle::R,
                g: vk::ComponentSwizzle::G,
                b: vk::ComponentSwizzle::B,
                a: vk::ComponentSwizzle::A,
            },
            subresource_range: vk::ImageSubresourceRange {
                aspect_mask: vk::ImageAspectFlags::COLOR,
                base_mip_level: 0,
                level_count: 1,
                base_array_layer: 0,
                layer_count: 1,
            },
        };

        let image_view = unsafe {
            device.logic.handle.create_image_view(&view_ci, None)
                .or(Err(VkError::create("Image View")))
        }?;

        let swapchain_image = SwapchainImage {
            image: image_handle,
            view : image_view,
        };
        result.push(swapchain_image);
    }

    Ok(result)
}



// -----------------------------------------------------------------------------------
fn query_optimal_present_mode(device: &VkDevice, surface: &VkSurface, config: &SwapchainConfig) -> VkResult<vk::PresentModeKHR> {

    // select a present mode for the swapchain.
    let available_modes = surface.query_present_modes(device.phy.handle)?;

    // The vk::PresentModeKHR::FIFO mode must always be present as per spec.
    // This mode waits for the vertical blank ("v-sync").
    let result = if config.present_vsync {
        vk::PresentModeKHR::FIFO
    } else {
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

fn query_swapchain_capability(device: &VkDevice, surface: &VkSurface, config: &SwapchainConfig) -> VkResult<SwapchainCapability> {

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
            width: min(max(config.dimension.width, surface_caps.min_image_extent.width), surface_caps.max_image_extent.width),
            height: min(max(config.dimension.height, surface_caps.min_image_extent.height), surface_caps.max_image_extent.height),
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
