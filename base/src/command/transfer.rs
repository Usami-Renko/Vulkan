
use ash::vk;
use ash::version::DeviceV1_0;

use crate::ci::sync::FenceCI;
use crate::ci::device::SubmitCI;

use crate::command::VkCommandType;
use crate::command::recorder::VkCmdRecorder;

use crate::utils::time::VkTimeDuration;
use crate::VkResult;

use crate::ci::image::ImageBarrierCI;

pub struct ITransfer;

impl VkCommandType for ITransfer {
    const BIND_POINT: vk::PipelineBindPoint = vk::PipelineBindPoint::GRAPHICS;
}

impl<'a> VkCmdRecorder<'a, ITransfer> {

    pub fn flush_copy_command(&self, queue: vk::Queue) -> VkResult<()> {

        let fence = self.device.build(&FenceCI::new(false))?;

        let submit_ci = SubmitCI::new()
            .add_command(self.command);
        self.device.submit(submit_ci, queue, fence)?;
        self.device.wait(fence, VkTimeDuration::Infinite)?;

        self.device.discard(fence);

        Ok(())
    }
}

impl<'a> CmdTransferApi for VkCmdRecorder<'a, ITransfer> {

    fn copy_buf2buf(&self, src: vk::Buffer, dst: vk::Buffer, regions: &[vk::BufferCopy]) -> &Self {
        unsafe {
            self.device.logic.handle.cmd_copy_buffer(self.command, src, dst, regions);
        } self
    }

    fn copy_buf2img(&self, src: vk::Buffer, dst: vk::Image, dst_layout: vk::ImageLayout, regions: &[vk::BufferImageCopy]) -> &Self {
        unsafe {
            self.device.logic.handle.cmd_copy_buffer_to_image(self.command, src, dst, dst_layout, regions);
        } self
    }

    fn copy_img2buf(&self, src: vk::Image, src_layout: vk::ImageLayout, dst: vk::Buffer, regions: &[vk::BufferImageCopy]) -> &Self {
        unsafe {
            self.device.logic.handle.cmd_copy_image_to_buffer(self.command, src, src_layout, dst, regions);
        } self
    }

    fn copy_img2img(&self,src: vk::Image, src_layout: vk::ImageLayout, dst: vk::Image, dst_layout: vk::ImageLayout, regions: &[vk::ImageCopy]) -> &Self {
        unsafe {
            self.device.logic.handle.cmd_copy_image(self.command, src, src_layout, dst, dst_layout, regions);
        } self
    }

    fn image_pipeline_barrier(&self, src_stage: vk::PipelineStageFlags, dst_stage: vk::PipelineStageFlags, dependencies: vk::DependencyFlags, image_barriers: Vec<ImageBarrierCI>) -> &Self {

        let barriers: Vec<vk::ImageMemoryBarrier> = image_barriers.into_iter()
            .map(|b| b.into()).collect();

        unsafe {
            self.device.logic.handle.cmd_pipeline_barrier(self.command, src_stage, dst_stage, dependencies, &[], &[], &barriers);
        } self
    }

    fn blit_image(&self, src_handle: vk::Image, src_layout: vk::ImageLayout, dst_handle: vk::Image, dst_layout: vk::ImageLayout, regions: &[vk::ImageBlit], filter: vk::Filter) -> &Self {
        unsafe {
            self.device.logic.handle.cmd_blit_image(self.command, src_handle, src_layout, dst_handle, dst_layout, regions, filter);
        } self
    }
}

pub trait CmdTransferApi {

    fn copy_buf2buf(&self, src_buffer_handle: vk::Buffer, dst_buffer_handle: vk::Buffer, regions: &[vk::BufferCopy]) -> &Self;

    fn copy_buf2img(&self, src_handle: vk::Buffer, dst_handle: vk::Image, dst_layout: vk::ImageLayout, regions: &[vk::BufferImageCopy]) -> &Self;

    fn copy_img2buf(&self, src_handle: vk::Image, src_layout: vk::ImageLayout, dst_buffer: vk::Buffer, regions: &[vk::BufferImageCopy]) -> &Self;

    fn copy_img2img(&self,src_handle: vk::Image, src_layout: vk::ImageLayout, dst_handle: vk::Image, dst_layout: vk::ImageLayout, regions: &[vk::ImageCopy]) -> &Self;

    fn image_pipeline_barrier(&self, src_stage: vk::PipelineStageFlags, dst_stage: vk::PipelineStageFlags, dependencies: vk::DependencyFlags, image_barriers: Vec<ImageBarrierCI>) -> &Self;

    fn blit_image(&self, src_handle: vk::Image, src_layout: vk::ImageLayout, dst_handle: vk::Image, dst_layout: vk::ImageLayout, regions: &[vk::ImageBlit], filter: vk::Filter) -> &Self;
}
