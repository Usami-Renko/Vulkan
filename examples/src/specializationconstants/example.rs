
use ash::vk;

use std::ptr;
use std::mem;
use std::path::Path;

use vkbase::context::{VkDevice, VkSwapchain};
use vkbase::ci::VkObjectBuildableCI;
use vkbase::ci::buffer::BufferCI;
use vkbase::ci::vma::{VmaBuffer, VmaAllocationCI};
use vkbase::ci::shader::{ShaderModuleCI, ShaderStageCI};
use vkbase::gltf::VkglTFModel;
use vkbase::texture::Texture2D;
use vkbase::context::VulkanContext;
use vkbase::{FlightCamera, FrameAction};
use vkbase::{vkbytes, vkuint, vkfloat, vkptr, Vec3F, Vec4F, Mat4F};
use vkbase::{VkResult, VkErrorKind};

use vkexamples::VkExampleBackend;

const VERTEX_SHADER_SOURCE_PATH  : &'static str = "examples/src/specializationconstants/uber.vert.glsl";
const FRAGMENT_SHADER_SOURCE_PATH: &'static str = "examples/src/specializationconstants/uber.frag.glsl";
const MODEL_PATH  : &'static str = "assets/models/color_teapot_spheres.gltf";
const TEXTURE_PATH: &'static str = "assets/textures/metalplate_nomips_rgba.ktx";


pub struct VulkanExample {

    backend: VkExampleBackend,

    model: VkglTFModel,
    color_map: Texture2D,
    ubo_buffer: VmaBuffer,

    pipelines: PipelineStaff,
    descriptors: DescriptorStaff,

    ubo_data: UboVS,
    camera: FlightCamera,

    is_toggle_event: bool,
}

struct PipelineStaff {
    phong     : vk::Pipeline,
    toon      : vk::Pipeline,
    textured  : vk::Pipeline,
    layout: vk::PipelineLayout,
}

impl VulkanExample {

    pub fn new(context: &mut VulkanContext) -> VkResult<VulkanExample> {

        let device = &mut context.device;
        let swapchain = &context.swapchain;
        let dimension = swapchain.dimension;

        let mut camera = FlightCamera::new()
            .place_at(Vec3F::new(12.0, 16.0, 0.2))
            .screen_aspect_ratio((dimension.width as f32 / 3.0) / dimension.height as f32)
            .pitch(-50.0)
            .yaw(0.0)
            .build();
        camera.set_move_speed(25.0);

        let ubo_data = UboVS {
            projection : camera.proj_matrix(),
            model      : camera.view_matrix(),
            light_pos  : Vec4F::new(-3.0, -12.0, 30.0, 0.0),
        };

        let render_pass = setup_renderpass(device, &context.swapchain)?;
        let backend = VkExampleBackend::new(device, swapchain, render_pass)?;

        let model = prepare_model(device)?;
        let color_map = Texture2D::load_ktx(device, Path::new(TEXTURE_PATH), vk::Format::R8G8B8A8_UNORM)?;
        let ubo_buffer = prepare_uniform(device)?;
        let descriptors = setup_descriptor(device, &ubo_buffer, &model, &color_map)?;

        let pipelines = prepare_pipelines(device, &model, backend.render_pass, descriptors.layout)?;

        let target = VulkanExample {
            backend, model, color_map, ubo_buffer, descriptors, pipelines, camera, ubo_data,
            is_toggle_event: true,
        };
        Ok(target)
    }
}

impl vkbase::RenderWorkflow for VulkanExample {

    fn init(&mut self, device: &VkDevice) -> VkResult<()> {

        self.backend.set_basic_ui(device, super::WINDOW_TITLE)?;

        self.update_uniforms()?;
        self.record_commands(device, self.backend.dimension)?;

        Ok(())
    }

    fn render_frame(&mut self, device: &mut VkDevice, device_available: vk::Fence, await_present: vk::Semaphore, image_index: usize, _delta_time: f32) -> VkResult<vk::Semaphore> {

        self.update_uniforms()?;

        let submit_ci = vkbase::ci::device::SubmitCI::new()
            .add_wait(vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT, await_present)
            .add_command(self.backend.commands[image_index])
            .add_signal(self.backend.await_rendering);

        // Submit to the graphics queue passing a wait fence.
        device.submit(submit_ci, device.logic.queues.graphics.handle, Some(device_available))?;

        Ok(self.backend.await_rendering)
    }

    fn swapchain_reload(&mut self, device: &mut VkDevice, new_chain: &VkSwapchain) -> VkResult<()> {

        // recreate the resources.
        device.discard(self.pipelines.phong);
        device.discard(self.pipelines.toon);
        device.discard(self.pipelines.textured);

        let render_pass = setup_renderpass(device, new_chain)?;
        self.backend.swapchain_reload(device, new_chain, render_pass)?;
        self.pipelines = prepare_pipelines(device, &self.model, self.backend.render_pass, self.descriptors.layout)?;

        self.record_commands(device, self.backend.dimension)?;

        Ok(())
    }

    fn receive_input(&mut self, inputer: &vkbase::EventController, delta_time: f32) -> FrameAction {

        if inputer.is_key_active() || inputer.is_cursor_active() {

            if inputer.key.is_key_pressed(winit::VirtualKeyCode::Escape) {
                return FrameAction::Terminal
            }

            self.is_toggle_event = true;
            self.camera.receive_input(inputer, delta_time);
        } else {
            self.is_toggle_event = false;
        }

        self.backend.update_fps_text(inputer);

        FrameAction::Rendering
    }

    fn deinit(self, device: &mut VkDevice) -> VkResult<()> {

        device.discard(self.descriptors.layout);
        device.discard(self.descriptors.pool);

        device.discard(self.pipelines.phong);
        device.discard(self.pipelines.toon);
        device.discard(self.pipelines.textured);
        device.discard(self.pipelines.layout);

        device.vma_discard(self.ubo_buffer)?;
        device.vma_discard(self.model)?;

        self.color_map.discard_by(device)?;
        self.backend.discard_by(device)
    }
}

impl VulkanExample {

    fn record_commands(&self, device: &VkDevice, dimension: vk::Extent2D) -> VkResult<()> {

        let scissor = vk::Rect2D {
            extent: dimension.clone(),
            offset: vk::Offset2D { x: 0, y: 0 },
        };

        for (i, &command) in self.backend.commands.iter().enumerate() {

            use vkbase::command::{VkCmdRecorder, CmdGraphicsApi, IGraphics};
            use vkbase::ci::pipeline::RenderPassBI;

            let render_params = vkbase::gltf::ModelRenderParams {
                descriptor_set : self.descriptors.set,
                pipeline_layout: self.pipelines.layout,
                material_stage : Some(vk::ShaderStageFlags::VERTEX),
            };

            let mut viewport = vk::Viewport {
                x: 0.0, y: 0.0,
                width: dimension.width as f32, height: dimension.height as f32,
                min_depth: 0.0, max_depth: 1.0,
            };

            let recorder: VkCmdRecorder<IGraphics> = VkCmdRecorder::new(&device.logic, command);

            let render_pass_bi = RenderPassBI::new(self.backend.render_pass, self.backend.framebuffers[i])
                .render_extent(dimension)
                .set_clear_values(vkexamples::DEFAULT_CLEAR_VALUES.clone());

            recorder.begin_record()?
                .begin_render_pass(render_pass_bi)
                .set_scissor(0, &[scissor]);

            { // Left
                viewport.width = dimension.width as f32 / 3.0;
                recorder
                    .set_viewport(0, &[viewport])
                    .bind_pipeline(self.pipelines.phong);
                self.model.record_command(&recorder, &render_params);
            }

            { // Center
                viewport.x = dimension.width as f32 / 3.0;
                recorder
                    .set_viewport(0, &[viewport])
                    .bind_pipeline(self.pipelines.toon);

                self.model.record_command(&recorder, &render_params);
            }

            { // Right
                viewport.x = dimension.width as f32 / 3.0 * 2.0;
                recorder
                    .set_viewport(0, &[viewport])
                    .bind_pipeline(self.pipelines.textured);
                self.model.record_command(&recorder, &render_params);
            }

            self.backend.ui_renderer.record_command(&recorder);

            recorder
                .end_render_pass()
                .end_record()?;
        }

        Ok(())
    }

    fn update_uniforms(&mut self) -> VkResult<()> {

        if self.is_toggle_event {

            self.ubo_data.model = self.camera.view_matrix();

            unsafe {
                let data_ptr = self.ubo_buffer.info.get_mapped_data() as vkptr<UboVS>;
                data_ptr.copy_from_nonoverlapping(&self.ubo_data, 1);
            }
        }

        Ok(())
    }
}

// Prepare model from glTF file.
pub fn prepare_model(device: &mut VkDevice) -> VkResult<VkglTFModel> {

    use vkbase::gltf::{GltfModelInfo, load_gltf};
    use vkbase::gltf::{AttributeFlags, NodeAttachmentFlags};

    let model_info = GltfModelInfo {
        path: Path::new(MODEL_PATH),
        // specify model's vertices layout.
        attribute: AttributeFlags::POSITION | AttributeFlags::NORMAL | AttributeFlags::TEXCOORD_0,
        // specify model's node attachment layout.
        node: NodeAttachmentFlags::TRANSFORM_MATRIX,
        transform: None,
    };

    let model = load_gltf(device, model_info)?;
    Ok(model)
}


// The uniform data that will be transferred to shader.
//
// layout (set = 0, binding = 0) uniform UBO {
//     mat4 projection;
//     mat4 model;
//     vec4 lightPos;
// } ubo;
#[derive(Debug, Clone, Copy)]
#[repr(C)]
struct UboVS {
    projection: Mat4F,
    model     : Mat4F,
    light_pos : Vec4F,
}

fn prepare_uniform(device: &mut VkDevice) -> VkResult<VmaBuffer> {

    let uniform_buffer = {
        let uniform_ci = BufferCI::new(mem::size_of::<[UboVS; 1]>() as vkbytes)
            .usage(vk::BufferUsageFlags::UNIFORM_BUFFER);
        let allocation_ci = VmaAllocationCI::new(vma::MemoryUsage::CpuOnly, vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT)
            .flags(vma::AllocationCreateFlags::MAPPED);
        let uniform_allocation = device.vma.create_buffer(uniform_ci.as_ref(), allocation_ci.as_ref())
            .map_err(VkErrorKind::Vma)?;
        VmaBuffer::from(uniform_allocation)
    };

    Ok(uniform_buffer)
}

struct DescriptorStaff {
    pool   : vk::DescriptorPool,
    set    : vk::DescriptorSet,
    layout : vk::DescriptorSetLayout,
}

fn setup_descriptor(device: &VkDevice, ubo_buffer: &VmaBuffer, model: &VkglTFModel, color_map: &Texture2D) -> VkResult<DescriptorStaff> {

    use vkbase::ci::descriptor::{DescriptorPoolCI, DescriptorSetLayoutCI};
    use vkbase::ci::descriptor::{DescriptorSetAI, DescriptorBufferSetWI, DescriptorImageSetWI, DescriptorSetsUpdateCI};

    // Descriptor Pool.
    let descriptor_pool = DescriptorPoolCI::new(1)
        .add_descriptor(vk::DescriptorType::UNIFORM_BUFFER, 1)
        .add_descriptor(vk::DescriptorType::UNIFORM_BUFFER_DYNAMIC, 1)
        .add_descriptor(vk::DescriptorType::COMBINED_IMAGE_SAMPLER, 1)
        .build(device)?;

    // in uber.vert.glsl:
    //
    // layout (set = 0, binding = 0) uniform UBO {
    //     mat4 projection;
    //     mat4 view;
    //     vec4 lightPos;
    // } ubo;
    let ubo_descriptor = vk::DescriptorSetLayoutBinding {
        binding: 0,
        descriptor_type: vk::DescriptorType::UNIFORM_BUFFER,
        descriptor_count: 1,
        stage_flags: vk::ShaderStageFlags::VERTEX,
        p_immutable_samplers: ptr::null(),
    };

    // in uber.vert.glsl
    //
    // layout (set = 0, binding = 1) uniform DynNode {
    //     mat4 transform;
    // } dyn_node;
    let node_descriptor = vk::DescriptorSetLayoutBinding {
        binding: 1,
        descriptor_type: vk::DescriptorType::UNIFORM_BUFFER_DYNAMIC,
        descriptor_count: 1,
        stage_flags: vk::ShaderStageFlags::VERTEX,
        p_immutable_samplers: ptr::null(),
    };

    // in uber.frag.glsl
    //
    // layout (binding = 2) uniform sampler2D samplerColormap;
    let sampler_handles = [color_map.sampler];
    let sampler_descriptor = vk::DescriptorSetLayoutBinding {
        binding: 2,
        descriptor_type: vk::DescriptorType::COMBINED_IMAGE_SAMPLER,
        descriptor_count: 1,
        stage_flags: vk::ShaderStageFlags::FRAGMENT,
        p_immutable_samplers: sampler_handles.as_ptr(),
    };

    let set_layout = DescriptorSetLayoutCI::new()
        .add_binding(ubo_descriptor)
        .add_binding(node_descriptor)
        .add_binding(sampler_descriptor)
        .build(device)?;

    // Descriptor set.
    let mut descriptor_sets = DescriptorSetAI::new(descriptor_pool)
        .add_set_layout(set_layout)
        .build(device)?;
    let descriptor_set = descriptor_sets.remove(0);

    let ubo_write_info = DescriptorBufferSetWI::new(descriptor_set, 0, vk::DescriptorType::UNIFORM_BUFFER)
        .add_buffer(vk::DescriptorBufferInfo {
            buffer: ubo_buffer.handle,
            offset: 0,
            range : mem::size_of::<UboVS>() as vkbytes,
        });
    let node_write_info = DescriptorBufferSetWI::new(descriptor_set, 1, vk::DescriptorType::UNIFORM_BUFFER_DYNAMIC)
        .add_buffer(model.nodes.node_descriptor());
    let sampler_write_info = DescriptorImageSetWI::new(descriptor_set, 2, vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
        .add_image(color_map.descriptor);

    DescriptorSetsUpdateCI::new()
        .add_write(&ubo_write_info)
        .add_write(&node_write_info)
        .add_write(&sampler_write_info)
        .update(device);

    let descriptors = DescriptorStaff {
        pool   : descriptor_pool,
        set    : descriptor_set,
        layout : set_layout,
    };
    Ok(descriptors)
}

fn setup_renderpass(device: &VkDevice, swapchain: &VkSwapchain) -> VkResult<vk::RenderPass> {

    use vkbase::ci::pipeline::RenderPassCI;
    use vkbase::ci::pipeline::{AttachmentDescCI, SubpassDescCI, SubpassDependencyCI};

    let color_attachment = AttachmentDescCI::new(swapchain.backend_format)
        .op(vk::AttachmentLoadOp::CLEAR, vk::AttachmentStoreOp::STORE)
        .layout(vk::ImageLayout::UNDEFINED, vk::ImageLayout::PRESENT_SRC_KHR);

    let depth_attachment = AttachmentDescCI::new(device.phy.depth_format)
        .op(vk::AttachmentLoadOp::CLEAR, vk::AttachmentStoreOp::DONT_CARE)
        .layout(vk::ImageLayout::UNDEFINED, vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL);

    let subpass_description = SubpassDescCI::new(vk::PipelineBindPoint::GRAPHICS)
        .add_color_attachment(0, vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL) // Attachment 0 is color.
        .set_depth_stencil_attachment(1, vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL); // Attachment 1 is depth-stencil.

    let dependency0 = SubpassDependencyCI::new(vk::SUBPASS_EXTERNAL, 0)
        .stage_mask(vk::PipelineStageFlags::BOTTOM_OF_PIPE, vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT)
        .access_mask(vk::AccessFlags::MEMORY_READ, vk::AccessFlags::COLOR_ATTACHMENT_READ | vk::AccessFlags::COLOR_ATTACHMENT_WRITE)
        .flags(vk::DependencyFlags::BY_REGION);

    let dependency1 = SubpassDependencyCI::new(0, vk::SUBPASS_EXTERNAL)
        .stage_mask(vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT, vk::PipelineStageFlags::BOTTOM_OF_PIPE)
        .access_mask(vk::AccessFlags::COLOR_ATTACHMENT_READ | vk::AccessFlags::COLOR_ATTACHMENT_WRITE, vk::AccessFlags::MEMORY_READ)
        .flags(vk::DependencyFlags::BY_REGION);

    let render_pass = RenderPassCI::new()
        .add_attachment(color_attachment)
        .add_attachment(depth_attachment)
        .add_subpass(subpass_description)
        .add_dependency(dependency0)
        .add_dependency(dependency1)
        .build(device)?;

    Ok(render_pass)
}

fn prepare_pipelines(device: &VkDevice, model: &VkglTFModel, render_pass: vk::RenderPass, set_layout: vk::DescriptorSetLayout) -> VkResult<PipelineStaff> {

    use vkbase::ci::pipeline::*;

    let viewport_state = ViewportSCI::new()
        .add_viewport(vk::Viewport::default())
        .add_scissor(vk::Rect2D::default());

    let rasterization_state = RasterizationSCI::new()
        .polygon(vk::PolygonMode::FILL)
        .cull_face(vk::CullModeFlags::BACK, vk::FrontFace::CLOCKWISE);

    let blend_attachment = BlendAttachmentSCI::new();
    let blend_state = ColorBlendSCI::new()
        .add_attachment(blend_attachment);

    let depth_stencil_state = DepthStencilSCI::new()
        .depth_test(true, true, vk::CompareOp::LESS_OR_EQUAL);

    let dynamic_state = DynamicSCI::new()
        .add_dynamic(vk::DynamicState::VIEWPORT)
        .add_dynamic(vk::DynamicState::SCISSOR);


    let material_range = vk::PushConstantRange {
        stage_flags: vk::ShaderStageFlags::VERTEX,
        offset: 0,
        size: model.materials.material_size(),
    };

    // Pipeline Layout.
    let pipeline_layout = PipelineLayoutCI::new()
        .add_set_layout(set_layout)
        .add_push_constants(material_range)
        .build(device)?;

    // base pipeline.
    let mut pipeline_ci = GraphicsPipelineCI::new(render_pass, pipeline_layout);

    pipeline_ci.set_vertex_input(model.meshes.vertex_input.clone());
    pipeline_ci.set_viewport(viewport_state);
    pipeline_ci.set_rasterization(rasterization_state.clone());
    pipeline_ci.set_depth_stencil(depth_stencil_state);
    pipeline_ci.set_color_blend(blend_state);
    pipeline_ci.set_dynamic(dynamic_state);


    // Prepare specialization data. -------------------------------------------------
    /// Host data to take specialization constants from.
    #[repr(C)]
    struct SpecializationData {
        /// Sets the lighting model used in the fragment "uber" shader.
        light_model: vkuint,
        /// Parameter for the toon shading part of the fragment shader.
        toon_desaturation_factor: vkfloat,
    }

    // Each shader constant of a shader stage corresponds to one map entry.

    // Shader bindings based on specialization constants are marked by the new "constant_id" layout qualifier:
    //     layout (constant_id = 0) const int LIGHTING_MODEL = 0;
    //	   layout (constant_id = 1) const float PARAM_TOON_DESATURATION = 0.0f;
    let map_entries = [
        // Map entry for the lighting model to be used by the fragment shader.
        vk::SpecializationMapEntry {
            constant_id: 0,
            offset: memoffset::offset_of!(SpecializationData, light_model) as vkuint,
            size: ::std::mem::size_of::<vkuint>(),
        },
        // Map entry for the toon shader parameter.
        vk::SpecializationMapEntry {
            constant_id: 1,
            offset: memoffset::offset_of!(SpecializationData, toon_desaturation_factor) as vkuint,
            size: ::std::mem::size_of::<vkfloat>(),
        },
    ];

    // Prepare specialization info block for the shader stage.
    let mut specialization_info = vk::SpecializationInfo {
        map_entry_count: map_entries.len() as _,
        p_map_entries  : map_entries.as_ptr(),
        data_size: ::std::mem::size_of::<SpecializationData>(),
        p_data: ptr::null(), // p_data will be set latter.
    };
    // ------------------------------------------------------------------------------


    // All pipelines will use the same "uber" shader and specialization constants to change branching and parameters of that shader
    let mut shader_compiler = vkbase::utils::shaderc::VkShaderCompiler::new()?;

    let vert_codes = shader_compiler.compile_from_path(Path::new(VERTEX_SHADER_SOURCE_PATH), shaderc::ShaderKind::Vertex, "[Vertex Shader]", "main")?;
    let frag_codes = shader_compiler.compile_from_path(Path::new(FRAGMENT_SHADER_SOURCE_PATH), shaderc::ShaderKind::Fragment, "[Fragment Shader]", "main")?;

    let vert_module = ShaderModuleCI::new(vert_codes)
        .build(device)?;
    let frag_module = ShaderModuleCI::new(frag_codes).build(device)?;

    // Create pipelines
    let phong_pipeline = {

        let specialization_data = SpecializationData {
            light_model: 0,
            toon_desaturation_factor: 0.5,
        };
        specialization_info.p_data = &specialization_data as *const SpecializationData as _;

        // Specialization info is assigned is part of the shader stage (module)
        // and must be set after creating the module and before creating the pipeline.
        let shaders = [
            ShaderStageCI::new(vk::ShaderStageFlags::VERTEX, vert_module),
            ShaderStageCI::new(vk::ShaderStageFlags::FRAGMENT, frag_module)
                .specialization(specialization_info),
        ];
        pipeline_ci.set_shaders(&shaders);

        device.build(&pipeline_ci)?
    };

    let toon_pipeline = {

        let specialization_data = SpecializationData {
            light_model: 1,
            toon_desaturation_factor: 0.5,
        };
        specialization_info.p_data = &specialization_data as *const SpecializationData as _;

        let shaders = [
            ShaderStageCI::new(vk::ShaderStageFlags::VERTEX, vert_module),
            ShaderStageCI::new(vk::ShaderStageFlags::FRAGMENT, frag_module)
                .specialization(specialization_info),
        ];
        pipeline_ci.set_shaders(&shaders);

        device.build(&pipeline_ci)?
    };

    let textured_pipeline = {

        let specialization_data = SpecializationData {
            light_model: 2,
            toon_desaturation_factor: 0.5,
        };
        specialization_info.p_data = &specialization_data as *const SpecializationData as _;

        let shaders = [
            ShaderStageCI::new(vk::ShaderStageFlags::VERTEX, vert_module),
            ShaderStageCI::new(vk::ShaderStageFlags::FRAGMENT, frag_module)
                .specialization(specialization_info),
        ];
        pipeline_ci.set_shaders(&shaders);

        device.build(&pipeline_ci)?
    };


    device.discard(vert_module);
    device.discard(frag_module);

    let result = PipelineStaff {
        phong: phong_pipeline,
        toon : toon_pipeline,
        textured: textured_pipeline,

        layout: pipeline_layout,
    };
    Ok(result)
}

