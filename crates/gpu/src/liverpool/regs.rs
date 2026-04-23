//! GPU register definitions for Liverpool (GCN 1.1).
//!
//! Tracks the state of all GPU registers that are set via PM4 commands.
//! These represent rendering state like render targets, shaders,
//! viewport, scissor, blend, depth, etc.

/// The complete GPU register state.
#[derive(Debug, Default)]
pub struct GpuRegs {
    /// Context registers (0xA000 - 0xAFFF range).
    pub context: ContextRegs,
    /// Shader registers (0x2C00 - 0x2FFF range).
    pub shader: ShaderRegs,
    /// Number of instances for instanced rendering.
    pub num_instances: u32,
    /// Index buffer base address.
    pub index_base: u64,
    /// Index buffer size.
    pub index_size: u32,
    /// Index type (16-bit or 32-bit).
    pub index_type: u32,
}

/// Context registers (render state).
#[derive(Debug, Default)]
pub struct ContextRegs {
    /// Color buffer 0-7 base addresses.
    pub cb_color_base: [u32; 8],
    /// Color buffer 0-7 sizes.
    pub cb_color_size: [u32; 8],
    /// Color buffer 0-7 info.
    pub cb_color_info: [u32; 8],
    /// Depth buffer Z info.
    pub db_z_info: u32,
    /// Depth buffer stencil info.
    pub db_stencil_info: u32,
    /// Viewport scale X.
    pub pa_cl_vport_xscale: f32,
    /// Viewport scale Y.
    pub pa_cl_vport_yscale: f32,
    /// Viewport scale Z.
    pub pa_cl_vport_zscale: f32,
    /// Viewport offset X.
    pub pa_cl_vport_xoffset: f32,
    /// Viewport offset Y.
    pub pa_cl_vport_yoffset: f32,
    /// Viewport offset Z.
    pub pa_cl_vport_zoffset: f32,
    /// Scissor top-left.
    pub pa_sc_screen_scissor_tl: u32,
    /// Scissor bottom-right.
    pub pa_sc_screen_scissor_br: u32,
}

/// Shader program registers.
#[derive(Debug, Default)]
pub struct ShaderRegs {
    /// Vertex shader program address.
    pub vs_program_addr: u64,
    /// Pixel shader program address.
    pub ps_program_addr: u64,
    /// Compute shader program address.
    pub cs_program_addr: u64,
    /// Geometry shader program address.
    pub gs_program_addr: u64,
    /// Hull shader program address.
    pub hs_program_addr: u64,
    /// VS user data registers (16 slots).
    pub vs_user_data: [u32; 16],
    /// PS user data registers (16 slots).
    pub ps_user_data: [u32; 16],
    /// CS user data registers (16 slots).
    pub cs_user_data: [u32; 16],
}

impl GpuRegs {
    /// Write a context register.
    pub fn write_context_reg(&mut self, offset: u32, value: u32) {
        tracing::trace!(offset = format!("0x{:04X}", offset), value = format!("0x{:08X}", value), "Context reg write");
        // Register decoding will be implemented when we process actual commands
    }

    /// Write a shader register.
    pub fn write_sh_reg(&mut self, offset: u32, value: u32) {
        tracing::trace!(offset = format!("0x{:04X}", offset), value = format!("0x{:08X}", value), "SH reg write");
    }

    /// Write a config register.
    pub fn write_config_reg(&mut self, offset: u32, value: u32) {
        tracing::trace!(offset = format!("0x{:04X}", offset), value = format!("0x{:08X}", value), "Config reg write");
    }

    /// Write a uconfig register.
    pub fn write_uconfig_reg(&mut self, offset: u32, value: u32) {
        tracing::trace!(offset = format!("0x{:04X}", offset), value = format!("0x{:08X}", value), "UConfig reg write");
    }
}
