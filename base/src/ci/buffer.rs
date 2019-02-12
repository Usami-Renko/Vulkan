
use ash::vk;
use ash::version::DeviceV1_0;

use crate::context::{VkDevice, VkObjectCreatable};
use crate::ci::{VulkanCI, VkObjectBuildableCI};
use crate::error::{VkResult, VkError};
use crate::{vkuint, vkbytes};

use std::ptr;

// ----------------------------------------------------------------------------------------------
/// Wrapper class for vk::BufferCreateInfo.
#[derive(Debug, Clone)]
pub struct BufferCI {
    ci: vk::BufferCreateInfo,
    queue_families: Vec<vkuint>,
}

impl VulkanCI for BufferCI {
    type CIType = vk::BufferCreateInfo;

    fn default_ci() -> Self::CIType {

        vk::BufferCreateInfo {
            s_type: vk::StructureType::BUFFER_CREATE_INFO,
            p_next: ptr::null(),
            flags : vk::BufferCreateFlags::empty(),
            size  : 0,
            usage : vk::BufferUsageFlags::empty(),
            sharing_mode: vk::SharingMode::EXCLUSIVE,
            queue_family_index_count: 0,
            p_queue_family_indices  : ptr::null(),
        }
    }
}

impl VkObjectBuildableCI for BufferCI {
    type ObjectType = (vk::Buffer, vk::MemoryRequirements);

    fn build(&self, device: &VkDevice) -> VkResult<Self::ObjectType> {

        let buffer = unsafe {
            device.logic.handle.create_buffer(&self.ci, None)
                .map_err(|_| VkError::create("Buffer"))?
        };

        let requirement = unsafe {
            device.logic.handle.get_buffer_memory_requirements(buffer)
        };

        Ok((buffer, requirement))
    }
}

impl BufferCI {

    pub fn new(size: vkbytes) -> BufferCI {

        BufferCI {
            ci: vk::BufferCreateInfo {
                size,
                ..BufferCI::default_ci()
            },
            queue_families: Vec::new(),
        }
    }

    pub fn flags(mut self, flags: vk::BufferCreateFlags) -> BufferCI {
        self.ci.flags = flags; self
    }

    pub fn usage(mut self, flags: vk::BufferUsageFlags) -> BufferCI {
        self.ci.usage = flags; self
    }

    pub fn sharing_queues(mut self, mode: vk::SharingMode, families_indices: Vec<vkuint>) -> BufferCI {
        self.queue_families = families_indices;
        self.ci.sharing_mode = mode; self
    }
}

impl VkObjectCreatable for vk::Buffer {

    fn discard(self, device: &VkDevice) {
        unsafe {
            device.logic.handle.destroy_buffer(self, None)
        }
    }
}
// ----------------------------------------------------------------------------------------------