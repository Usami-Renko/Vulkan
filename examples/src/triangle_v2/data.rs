
use ash::vk;
use ash::version::DeviceV1_0;

use vkbase::context::VkDevice;
use vkbase::ci::buffer::BufferCI;
use vkbase::{VkResult, VkError};
use vkbase::{vkuint, vkbytes};

use crate::helper;

use std::mem;
use std::ptr;

type Mat4F = nalgebra::Matrix4<f32>;

/// Vertex layout used in this example.
#[derive(Debug, Clone, Copy)]
pub struct Vertex {
    position: [f32; 3],
    color: [f32; 3],
}

pub struct InputDescriptionStaff {
    pub bindings  : Vec<vk::VertexInputBindingDescription>,
    pub attributes: Vec<vk::VertexInputAttributeDescription>,
    pub state: vk::PipelineVertexInputStateCreateInfo,
}

impl Vertex {

    pub fn input_description() -> InputDescriptionStaff {

        // Vertex input binding
        // This example uses a single vertex input binding at binding point 0 (see vkCmdBindVertexBuffers).
        let input_bindings = vec![
            vk::VertexInputBindingDescription {
                binding: 0,
                stride : mem::size_of::<Vertex>() as _,
                input_rate: vk::VertexInputRate::VERTEX,
            },
        ];

        // Input attribute bindings describe shader attribute locations and memory layouts
        let vertex_input_attributes = vec![
            // layout (location = 0) in vec3 inPos;
            vk::VertexInputAttributeDescription {
                location: 0,
                binding : 0,
                format  : vk::Format::R32G32B32_SFLOAT, // three 32 bit signed (SFLOAT) floats (R32 G32 B32).
                offset  : memoffset::offset_of!(Vertex, position) as _,
            },
            // layout (location = 1) in vec3 inColor;
            vk::VertexInputAttributeDescription {
                location: 1,
                binding : 0,
                format  : vk::Format::R32G32B32_SFLOAT,
                offset  : memoffset::offset_of!(Vertex, color) as _,
            },
        ];

        // Vertex input state used for pipeline creation
        let input_state = vk::PipelineVertexInputStateCreateInfo {
            s_type: vk::StructureType::PIPELINE_VERTEX_INPUT_STATE_CREATE_INFO,
            p_next: ptr::null(),
            flags : vk::PipelineVertexInputStateCreateFlags::empty(),
            vertex_binding_description_count: input_bindings.len() as _,
            p_vertex_binding_descriptions   : input_bindings.as_ptr(),
            vertex_attribute_description_count: vertex_input_attributes.len() as _,
            p_vertex_attribute_descriptions   : vertex_input_attributes.as_ptr(),
        };

        InputDescriptionStaff {
            bindings   : input_bindings,
            attributes : vertex_input_attributes,
            state      : input_state,
        }
    }
}

/// Vertex buffer.
pub struct VertexBuffer {
    /// handle to the device memory of current vertex buffer.
    pub memory: vk::DeviceMemory,
    /// handle to the vk::Buffer object that the memory is bound to.
    pub buffer: vk::Buffer,
}

/// Index Buffer.
pub struct IndexBuffer {
    pub memory: vk::DeviceMemory,
    pub buffer: vk::Buffer,
    /// The element count of indices used in this index buffer.
    pub count: vkuint,
}

/// Uniform buffer block object.
pub struct UniformBuffer {
    pub memory: vk::DeviceMemory,
    pub buffer: vk::Buffer,
    pub descriptor: vk::DescriptorBufferInfo,
}

// The uniform data that will be transferred to shader.
//
//	layout(set = 0, binding = 0) uniform UBO {
//		mat4 projectionMatrix;
//		mat4 viewMatrix;
//		mat4 modelMatrix;
//	} ubo;
#[derive(Debug, Clone, Copy)]
pub struct UboVS {
    pub projection: Mat4F,
    pub view: Mat4F,
    pub model: Mat4F,
}

pub struct DepthImage {
    pub image: vk::Image,
    pub view : vk::ImageView,
    pub memory: vk::DeviceMemory,
}


// Prepare vertex buffer and index buffer for an indexed triangle.
pub fn prepare_vertices(device: &VkDevice, command_pool: vk::CommandPool) -> VkResult<(VertexBuffer, IndexBuffer)> {

    // A note on memory management in Vulkan in general:
    // This is a very complex topic and while it's fine for an example application to to small individual memory allocations that is not
    // what should be done a real-world application, where you should allocate large chunks of memory at once instead.

    let vertices_data = [
        Vertex { position: [ 1.0,  1.0, 0.0], color: [1.0, 0.0, 0.0] },
        Vertex { position: [-1.0,  1.0, 0.0], color: [0.0, 1.0, 0.0] },
        Vertex { position: [ 0.0, -1.0, 0.0], color: [0.0, 0.0, 1.0] },
    ];
    let vertices = allocate_buffer(device, &vertices_data, vk::BufferUsageFlags::VERTEX_BUFFER)?;

    let indices_data = [0, 1, 2_u32];
    let indices = allocate_buffer(device, &indices_data, vk::BufferUsageFlags::INDEX_BUFFER)?;

    let copy_command = helper::create_command_buffer(device, command_pool, true)?;

    unsafe {

        let vertex_copy_region = vk::BufferCopy {
            src_offset: 0,
            dst_offset: 0,
            size: vertices.buffer_size,
        };
        device.logic.handle.cmd_copy_buffer(copy_command, vertices.staging_buffer, vertices.target_buffer, &[vertex_copy_region]);

        let index_copy_region = vk::BufferCopy {
            src_offset: 0,
            dst_offset: 0,
            size: indices.buffer_size,
        };
        device.logic.handle.cmd_copy_buffer(copy_command, indices.staging_buffer, indices.target_buffer, &[index_copy_region]);
    }

    // Flushing the command buffer will also submit it to the queue
    // and uses a fence to ensure that all commands have been executed before returning.
    helper::flush_command_buffer(device, command_pool, copy_command)?;

    // Destroy staging buffers
    device.discard(vertices.staging_buffer);
    device.discard(vertices.staging_memory);

    device.discard(indices.staging_buffer);
    device.discard(indices.staging_memory);

    let vertex_buffer = VertexBuffer {
        buffer: vertices.target_buffer,
        memory: vertices.target_memory,
    };

    let index_buffer = IndexBuffer {
        buffer: indices.target_buffer,
        memory: indices.target_memory,
        count: indices_data.len() as _,
    };

    Ok((vertex_buffer, index_buffer))
}


struct BufferResourceTmp {

    buffer_size: vkbytes,

    staging_buffer: vk::Buffer,
    staging_memory: vk::DeviceMemory,

    target_buffer: vk::Buffer,
    target_memory: vk::DeviceMemory,
}

fn allocate_buffer<D: Copy>(device: &VkDevice, data: &[D], buffer_usage: vk::BufferUsageFlags) -> VkResult<BufferResourceTmp> {

    let buffer_size = (mem::size_of::<D>() * data.len()) as vkbytes;

    let (staging_buffer, staging_memory_requirement) = BufferCI::new(buffer_size, vk::BufferUsageFlags::TRANSFER_SRC)
        .build(device)?;

    let staging_mem_alloc = vk::MemoryAllocateInfo {
        s_type: vk::StructureType::MEMORY_ALLOCATE_INFO,
        p_next: ptr::null(),
        allocation_size: staging_memory_requirement.size,
        memory_type_index: helper::get_memory_type_index(
            device, staging_memory_requirement.memory_type_bits,
            vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT),
    };

    let staging_memory = unsafe {
        device.logic.handle.allocate_memory(&staging_mem_alloc, None)
            .map_err(|_| VkError::create("Memory Allocate"))?
    };

    unsafe {

        // map and copy.
        let data_ptr = device.logic.handle.map_memory(staging_memory, 0, staging_mem_alloc.allocation_size, vk::MemoryMapFlags::empty())
            .map_err(|_| VkError::device("Map Memory"))?;

        let mapped_copy_target = ::std::slice::from_raw_parts_mut(data_ptr as *mut D, data.len());
        mapped_copy_target.copy_from_slice(data);

        device.logic.handle.unmap_memory(staging_memory);

        device.logic.handle.bind_buffer_memory(staging_buffer, staging_memory, 0)
            .map_err(|_| VkError::device("Binding Buffer Memory"))?;
    }



    let (target_buffer, target_memory_requirement) = BufferCI::new(buffer_size, vk::BufferUsageFlags::TRANSFER_DST | buffer_usage)
        .build(device)?;

    let target_mem_alloc = vk::MemoryAllocateInfo {
        allocation_size: target_memory_requirement.size,
        memory_type_index: helper::get_memory_type_index(device, target_memory_requirement.memory_type_bits, vk::MemoryPropertyFlags::DEVICE_LOCAL),
        ..staging_mem_alloc
    };

    let target_memory = unsafe {
        device.logic.handle.allocate_memory(&target_mem_alloc, None)
            .map_err(|_| VkError::create("Memory Allocate"))?
    };

    unsafe {
        device.logic.handle.bind_buffer_memory(target_buffer, target_memory, 0)
            .map_err(|_| VkError::device("Binding Buffer Memory"))?;
    }

    let result = BufferResourceTmp { buffer_size, staging_buffer, staging_memory, target_buffer, target_memory };
    Ok(result)
}

pub fn prepare_uniform(device: &VkDevice, dimension: vk::Extent2D) -> VkResult<UniformBuffer> {

    let (uniform_buffer, memory_requirement) = BufferCI::new(mem::size_of::<UboVS>() as vkbytes, vk::BufferUsageFlags::UNIFORM_BUFFER)
        .build(device)?;

    let mem_alloc = vk::MemoryAllocateInfo {
        s_type: vk::StructureType::MEMORY_ALLOCATE_INFO,
        p_next: ptr::null(),
        allocation_size: memory_requirement.size,
        memory_type_index: helper::get_memory_type_index(
            device, memory_requirement.memory_type_bits,
            vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT),
    };
    let uniform_memory = unsafe {
        device.logic.handle.allocate_memory(&mem_alloc, None)
            .map_err(|_| VkError::create("Memory Allocate"))?
    };

    unsafe {
        device.logic.handle.bind_buffer_memory(uniform_buffer, uniform_memory, 0)
            .map_err(|_| VkError::device("Binding Buffer Memory"))?
    };

    let descriptor_info = vk::DescriptorBufferInfo {
        buffer: uniform_buffer,
        offset: 0,
        range: mem::size_of::<UboVS>() as vkbytes,
    };

    let result = UniformBuffer {
        buffer: uniform_buffer,
        memory: uniform_memory,
        descriptor: descriptor_info,
    };

    update_uniform_buffers(device, dimension, &result)?;

    Ok(result)
}

fn update_uniform_buffers(device: &VkDevice, dimension: vk::Extent2D, uniforms: &UniformBuffer) -> VkResult<()> {

    let screen_aspect = (dimension.width as f32) / (dimension.height as f32);

    let ubo_data = [
        UboVS {
            projection: Mat4F::new_perspective(screen_aspect, 60.0_f32.to_radians(), 0.1, 256.0),
            view: Mat4F::new_translation(&nalgebra::Vector3::new(0.0, 0.0, -2.5)),
            model: Mat4F::identity(),
        },
    ];

    // Map uniform buffer and update it.
    unsafe {
        let data_ptr = device.logic.handle.map_memory(uniforms.memory, 0, mem::size_of::<UboVS>() as _, vk::MemoryMapFlags::empty())
            .map_err(|_| VkError::device("Map Memory"))?;

        let mapped_copy_target = ::std::slice::from_raw_parts_mut(data_ptr as *mut UboVS, ubo_data.len());
        mapped_copy_target.copy_from_slice(&ubo_data);

        device.logic.handle.unmap_memory(uniforms.memory);
    }

    Ok(())
}
