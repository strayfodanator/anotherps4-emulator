//! gpu liverpool to vulkan translation (wip)

pub mod liverpool;
pub mod renderer;
pub mod shader;

use liverpool::command_processor::CommandProcessor;
use renderer::VulkanRenderer;
use std::sync::{Arc, Mutex};

// core gpu context tying everything together
pub struct GpuContext {
    pub command_processor: CommandProcessor,
    pub renderer: Option<VulkanRenderer>,
    pub frames_submitted: u64,
}

impl GpuContext {
    /// Create a new GPU context with a Vulkan renderer attached to a window.
    pub fn new(window: Option<Arc<winit::window::Window>>) -> Self {
        let renderer = if let Some(win) = window {
            match VulkanRenderer::new(win) {
                Ok(r) => {
                    tracing::info!("GPU initialized with Vulkan on: {}", r.gpu_name());
                    Some(r)
                }
                Err(e) => {
                    tracing::warn!("Failed to init Vulkan: {}. Running without GPU rendering.", e);
                    None
                }
            }
        } else {
            tracing::warn!("No window provided. GPU rendering disabled.");
            None
        };

        GpuContext {
            command_processor: CommandProcessor::new(),
            renderer,
            frames_submitted: 0,
        }
    }

    /// Submit a graphics command buffer (DCB) for processing.
    /// This is what sceGnmSubmitCommandBuffers calls into.
    pub fn submit_gfx(&mut self, dcb: &[u32], _ccb: &[u32]) {
        self.command_processor.process_command_buffer(dcb);
    }

    /// Called by sceGnmSubmitDone — signals end of frame.
    pub fn submit_done(&mut self) {
        self.frames_submitted += 1;

        // Present a blank frame in the renderer (placeholder for Swapchain testing)
        if let Some(ref mut renderer) = self.renderer {
            // Placeholder color for frame swap
            renderer.present_blank_frame(0.0, 0.2, 0.6);
        }

        if self.frames_submitted % 60 == 0 {
            tracing::info!(
                "GPU: {} frames submitted, {} command buffers processed",
                self.frames_submitted,
                self.command_processor.submit_count
            );
        }
    }
}

impl Default for GpuContext {
    fn default() -> Self {
        Self::new(None)
    }
}

/// Global GPU context, accessible from HLE stubs.
static GPU_CONTEXT: Mutex<Option<GpuContext>> = Mutex::new(None);

/// Initialize the global GPU singleton. Must be called once at startup.
pub fn init_gpu(window: Option<Arc<winit::window::Window>>) {
    let mut lock = GPU_CONTEXT.lock().unwrap();
    if lock.is_none() {
        *lock = Some(GpuContext::new(window));
    }
}

/// Access the global GPU context for submitting commands.
pub fn with_gpu<F, R>(f: F) -> R
where
    F: FnOnce(&mut GpuContext) -> R,
{
    let mut lock = GPU_CONTEXT.lock().unwrap();
    let gpu = lock.get_or_insert_with(GpuContext::default);
    f(gpu)
}
