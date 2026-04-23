//! GPU command processor.
//!
//! Runs on a dedicated thread, processing PM4 command buffers submitted
//! by the guest via the GNM driver. Translates GPU commands into
//! Vulkan rendering operations.

use super::pm4::{self, Pm4Type};
use super::regs::GpuRegs;

/// The GPU command processor state.
pub struct CommandProcessor {
    /// GPU register state.
    pub regs: GpuRegs,
    /// Number of processed command buffers.
    pub submit_count: u64,
}

impl CommandProcessor {
    pub fn new() -> Self {
        tracing::info!("GPU Command Processor initialized");
        CommandProcessor {
            regs: GpuRegs::default(),
            submit_count: 0,
        }
    }

    /// Process a PM4 command buffer (Draw Command Buffer).
    pub fn process_command_buffer(&mut self, dcb: &[u32]) {
        let mut offset = 0;

        while offset < dcb.len() {
            let header_word = dcb[offset];
            let header = pm4::parse_pm4_header(header_word);

            match header.packet_type {
                Pm4Type::Type2 => {
                    // NOP padding — skip
                    offset += 1;
                }
                Pm4Type::Type3 => {
                    let body_dwords = header.count as usize + 1;
                    let end = (offset + 1 + body_dwords).min(dcb.len());
                    let body = &dcb[offset + 1..end];

                    if let Some(opcode) = header.opcode {
                        self.process_type3_packet(opcode, body);
                    } else {
                        tracing::warn!(
                            raw_opcode = (header_word >> 8) & 0xFF,
                            "Unknown PM4 Type3 opcode"
                        );
                    }

                    offset = end;
                }
                _ => {
                    offset += 1;
                }
            }
        }

        self.submit_count += 1;
    }

    /// Handle a Type3 (IT) PM4 packet.
    fn process_type3_packet(&mut self, opcode: pm4::Pm4Opcode, body: &[u32]) {
        match opcode {
            pm4::Pm4Opcode::Nop => {
                // Nothing to do
            }
            pm4::Pm4Opcode::SetContextReg => {
                if body.len() >= 2 {
                    let reg_offset = body[0];
                    for (i, &value) in body[1..].iter().enumerate() {
                        self.regs
                            .write_context_reg(reg_offset + i as u32, value);
                    }
                }
            }
            pm4::Pm4Opcode::SetShReg => {
                if body.len() >= 2 {
                    let reg_offset = body[0];
                    for (i, &value) in body[1..].iter().enumerate() {
                        self.regs.write_sh_reg(reg_offset + i as u32, value);
                    }
                }
            }
            pm4::Pm4Opcode::SetConfigReg => {
                if body.len() >= 2 {
                    let reg_offset = body[0];
                    for (i, &value) in body[1..].iter().enumerate() {
                        self.regs.write_config_reg(reg_offset + i as u32, value);
                    }
                }
            }
            pm4::Pm4Opcode::SetUconfigReg => {
                if body.len() >= 2 {
                    let reg_offset = body[0];
                    for (i, &value) in body[1..].iter().enumerate() {
                        self.regs.write_uconfig_reg(reg_offset + i as u32, value);
                    }
                }
            }
            pm4::Pm4Opcode::DrawIndex2 => {
                tracing::debug!(body_len = body.len(), "DRAW_INDEX_2 (stub)");
                // TODO: trigger Vulkan draw call
            }
            pm4::Pm4Opcode::DrawIndexAuto => {
                tracing::debug!(body_len = body.len(), "DRAW_INDEX_AUTO (stub)");
                // TODO: trigger Vulkan draw call
            }
            pm4::Pm4Opcode::DispatchDirect => {
                tracing::debug!(body_len = body.len(), "DISPATCH_DIRECT (stub)");
                // TODO: trigger Vulkan compute dispatch
            }
            pm4::Pm4Opcode::NumInstances => {
                if !body.is_empty() {
                    self.regs.num_instances = body[0];
                }
            }
            pm4::Pm4Opcode::IndexBase => {
                if body.len() >= 2 {
                    self.regs.index_base =
                        (body[0] as u64) | ((body[1] as u64) << 32);
                }
            }
            pm4::Pm4Opcode::IndexType => {
                if !body.is_empty() {
                    self.regs.index_type = body[0];
                }
            }
            pm4::Pm4Opcode::IndexBufferSize => {
                if !body.is_empty() {
                    self.regs.index_size = body[0];
                }
            }
            pm4::Pm4Opcode::EventWriteEop => {
                tracing::trace!("EVENT_WRITE_EOP (stub)");
                // TODO: GPU fence/event signaling
            }
            pm4::Pm4Opcode::AcquireMem => {
                tracing::trace!("ACQUIRE_MEM (stub)");
            }
            pm4::Pm4Opcode::WaitRegMem => {
                tracing::trace!("WAIT_REG_MEM (stub)");
            }
            pm4::Pm4Opcode::ContextControl => {
                tracing::trace!("CONTEXT_CONTROL");
            }
            pm4::Pm4Opcode::ClearState => {
                tracing::trace!("CLEAR_STATE");
            }
            _ => {
                tracing::trace!(opcode = ?opcode, "Unhandled PM4 opcode");
            }
        }
    }
}

impl Default for CommandProcessor {
    fn default() -> Self {
        Self::new()
    }
}
