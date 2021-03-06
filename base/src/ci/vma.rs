//! Utilities for using Vulkan Memory Allocator(vma).

use ash::vk;

use crate::ci::VulkanCI;
use crate::context::VmaResourceDiscardable;
use crate::{VkResult, VkErrorKind};
use crate::{vkuint, vkptr};

// ----------------------------------------------------------------------------------------------
/// A type contains the buffer allocation result from `vma::Allocator`.
#[derive(Debug, Clone)]
pub struct VmaBuffer {

    /// the handle of `vk::Buffer`.
    pub handle: vk::Buffer,
    /// allocation info managed by vma.
    pub allocation: vma::Allocation,
    /// the meta information about about this memory and allocation.
    pub info: vma::AllocationInfo,
}

impl From<(vk::Buffer, vma::Allocation, vma::AllocationInfo)> for VmaBuffer {

    fn from(content: (vk::Buffer, vma::Allocation, vma::AllocationInfo)) -> VmaBuffer {
        VmaBuffer {
            handle: content.0,
            allocation: content.1,
            info: content.2,
        }
    }
}

impl VmaResourceDiscardable for VmaBuffer {

    fn discard_by(self, vma: &mut vma::Allocator) -> VkResult<()> {
        vma.destroy_buffer(self.handle, &self.allocation)
            .map_err(VkErrorKind::Vma)?;
        Ok(())
    }
}

/// A type contains the image allocation result from `vma::Allocator`.
#[derive(Debug, Clone)]
pub struct VmaImage {

    /// the handle of `vk::Image`.
    pub handle: vk::Image,
    /// allocation info managed by vma.
    pub allocation: vma::Allocation,
    /// the meta information about about this memory and allocation.
    pub info: vma::AllocationInfo,
}


impl From<(vk::Image, vma::Allocation, vma::AllocationInfo)> for VmaImage {

    fn from(content: (vk::Image, vma::Allocation, vma::AllocationInfo)) -> VmaImage {
        VmaImage {
            handle: content.0,
            allocation: content.1,
            info: content.2,
        }
    }
}

impl VmaResourceDiscardable for VmaImage {

    fn discard_by(self, vma: &mut vma::Allocator) -> VkResult<()> {
        vma.destroy_image(self.handle, &self.allocation)
            .map_err(VkErrorKind::Vma)?;
        Ok(())
    }
}
// ----------------------------------------------------------------------------------------------


// ----------------------------------------------------------------------------------------------
/// Wrapper class for `vma::AllocationCreateInfo`.
///
/// The default values are defined as follows:
/// ``` ignore
/// vma::AllocationCreateInfo {
///     usage: vma::MemoryUsage::Unknown,
///     flags: vma::AllocationCreateFlags::NONE,
///     required_flags : vk::MemoryPropertyFlags::empty(),
///     preferred_flags: vk::MemoryPropertyFlags::empty(),
///     memory_type_bits: 0,
///     pool: None,
///     user_data: None,
/// }
/// ```
///
/// See [AllocationCreateInfo](https://docs.rs/vk-mem/0.1.6/vk_mem/struct.AllocationCreateInfo.html) for more detail.
///
#[derive(Debug, Clone)]
pub struct VmaAllocationCI {
    inner: vma::AllocationCreateInfo,
}

impl VulkanCI<vma::AllocationCreateInfo> for VmaAllocationCI {

    fn default_ci() -> vma::AllocationCreateInfo {

        vma::AllocationCreateInfo {
            usage: vma::MemoryUsage::Unknown,
            flags: vma::AllocationCreateFlags::NONE,
            required_flags : vk::MemoryPropertyFlags::empty(),
            preferred_flags: vk::MemoryPropertyFlags::empty(),
            // set `memory_type_bits` means to accept all memory type index.
            memory_type_bits: 0,
            pool: None,
            user_data: None,
        }
    }
}

impl AsRef<vma::AllocationCreateInfo> for VmaAllocationCI {

    fn as_ref(&self) -> &vma::AllocationCreateInfo {
        &self.inner
    }
}

impl VmaAllocationCI {

    /// Initialize `vma::AllocationCreateInfo` with default value.
    ///
    /// `usage` is the intended usage of memory.
    ///
    /// `required_flags` is the flags that must be set for Memory Type chosen.
    pub fn new(usage: vma::MemoryUsage, required_flags: vk::MemoryPropertyFlags) -> VmaAllocationCI {

        VmaAllocationCI {
            inner: vma::AllocationCreateInfo {
                usage, required_flags,
                ..VmaAllocationCI::default_ci()
            }
        }
    }

    /// Set the `flags` member for `vma::AllocationCreateInfo`.
    ///
    /// It specifies the configuration of allocation.
    #[inline(always)]
    pub fn flags(mut self, flags: vma::AllocationCreateFlags) -> VmaAllocationCI {
        self.inner.flags = flags; self
    }

    /// Set the `preferred_flags` member for `vma::AllocationCreateInfo`.
    ///
    /// It specifies the preferred flags for Memory Type chosen.
    #[inline(always)]
    pub fn preferred_flags(mut self, flags: vk::MemoryPropertyFlags) -> VmaAllocationCI {
        self.inner.preferred_flags = flags; self
    }

    /// Set the `memory_type_bits` member for `vma::AllocationCreateInfo`.
    ///
    /// It limits the acceptable memory type.
    #[inline(always)]
    pub fn accept_memory_types(mut self, acceptable_type_bits: vkuint) -> VmaAllocationCI {
        self.inner.memory_type_bits = acceptable_type_bits; self
    }

    /// Set the `pool` member for `vma::AllocationCreateInfo`.
    ///
    /// Only use this function for custom memory allocation.
    #[inline(always)]
    pub fn with_pool(mut self, pool: vma::AllocatorPool) -> VmaAllocationCI {
        self.inner.pool = Some(pool); self
    }

    /// Set the `user_data` member for `vma::AllocationCreateInfo`.
    #[inline(always)]
    pub fn with_user_data(mut self, data_ptr: vkptr) -> VmaAllocationCI {
        self.inner.user_data = Some(data_ptr); self
    }
}
// ----------------------------------------------------------------------------------------------
