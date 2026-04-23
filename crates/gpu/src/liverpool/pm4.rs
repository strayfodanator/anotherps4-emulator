//! PM4 (Packet Manager 4) command buffer parsing.
//!
//! PM4 is AMD's GPU command protocol. Games fill command buffers with
//! PM4 packets that specify draw calls, state changes, memory operations, etc.
//! We parse these packets and translate them into Vulkan operations.

use anotherps4_common::bitflags;

/// PM4 IT (Indirect Transfer) opcodes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum Pm4Opcode {
    Nop = 0x10,
    SetBase = 0x11,
    ClearState = 0x12,
    IndexBufferSize = 0x13,
    DispatchDirect = 0x15,
    DispatchIndirect = 0x16,
    AtomicGds = 0x1D,
    OcclusionQuery = 0x1F,
    SetPredication = 0x20,
    RegRmw = 0x21,
    DrawIndirect = 0x24,
    DrawIndexIndirect = 0x25,
    IndexBase = 0x26,
    DrawIndex2 = 0x27,
    ContextControl = 0x28,
    IndexType = 0x2A,
    DrawIndexAuto = 0x2D,
    NumInstances = 0x2F,
    DrawIndexMultiAuto = 0x30,
    IndirectBufferConst = 0x33,
    DrawIndexOffset2 = 0x35,
    WriteData = 0x37,
    MemSemaphore = 0x39,
    WaitRegMem = 0x3C,
    IndirectBuffer = 0x3F,
    CopyData = 0x40,
    PfpSyncMe = 0x42,
    SurfaceSync = 0x43,
    EventWrite = 0x46,
    EventWriteEop = 0x47,
    EventWriteEos = 0x48,
    ReleaseMem = 0x49,
    DmaData = 0x50,
    AcquireMem = 0x58,
    LoadShReg = 0x5F,
    LoadContextReg = 0x61,
    SetConfigReg = 0x68,
    SetContextReg = 0x69,
    SetShReg = 0x76,
    SetUconfigReg = 0x79,
    LoadConstRam = 0x80,
    WriteConstRam = 0x81,
    DumpConstRam = 0x83,
    IncrementCeCounter = 0x84,
    IncrementDeCounter = 0x85,
    WaitOnCeCounter = 0x86,
    WaitOnDeCounterDiff = 0x88,
}

impl Pm4Opcode {
    pub fn from_u32(value: u32) -> Option<Self> {
        match value {
            0x10 => Some(Self::Nop),
            0x11 => Some(Self::SetBase),
            0x12 => Some(Self::ClearState),
            0x13 => Some(Self::IndexBufferSize),
            0x15 => Some(Self::DispatchDirect),
            0x16 => Some(Self::DispatchIndirect),
            0x1D => Some(Self::AtomicGds),
            0x1F => Some(Self::OcclusionQuery),
            0x20 => Some(Self::SetPredication),
            0x21 => Some(Self::RegRmw),
            0x24 => Some(Self::DrawIndirect),
            0x25 => Some(Self::DrawIndexIndirect),
            0x26 => Some(Self::IndexBase),
            0x27 => Some(Self::DrawIndex2),
            0x28 => Some(Self::ContextControl),
            0x2A => Some(Self::IndexType),
            0x2D => Some(Self::DrawIndexAuto),
            0x2F => Some(Self::NumInstances),
            0x30 => Some(Self::DrawIndexMultiAuto),
            0x33 => Some(Self::IndirectBufferConst),
            0x35 => Some(Self::DrawIndexOffset2),
            0x37 => Some(Self::WriteData),
            0x39 => Some(Self::MemSemaphore),
            0x3C => Some(Self::WaitRegMem),
            0x3F => Some(Self::IndirectBuffer),
            0x40 => Some(Self::CopyData),
            0x42 => Some(Self::PfpSyncMe),
            0x43 => Some(Self::SurfaceSync),
            0x46 => Some(Self::EventWrite),
            0x47 => Some(Self::EventWriteEop),
            0x48 => Some(Self::EventWriteEos),
            0x49 => Some(Self::ReleaseMem),
            0x50 => Some(Self::DmaData),
            0x58 => Some(Self::AcquireMem),
            0x5F => Some(Self::LoadShReg),
            0x61 => Some(Self::LoadContextReg),
            0x68 => Some(Self::SetConfigReg),
            0x69 => Some(Self::SetContextReg),
            0x76 => Some(Self::SetShReg),
            0x79 => Some(Self::SetUconfigReg),
            0x80 => Some(Self::LoadConstRam),
            0x81 => Some(Self::WriteConstRam),
            0x83 => Some(Self::DumpConstRam),
            0x84 => Some(Self::IncrementCeCounter),
            0x85 => Some(Self::IncrementDeCounter),
            0x86 => Some(Self::WaitOnCeCounter),
            0x88 => Some(Self::WaitOnDeCounterDiff),
            _ => None,
        }
    }
}

/// PM4 packet header type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pm4Type {
    /// Type 0: register writes.
    Type0,
    /// Type 2: NOP filler.
    Type2,
    /// Type 3: IT (indirect transfer) commands.
    Type3,
}

/// A parsed PM4 packet header.
#[derive(Debug, Clone)]
pub struct Pm4Header {
    pub packet_type: Pm4Type,
    pub count: u16,
    pub opcode: Option<Pm4Opcode>,
}

/// Parse a PM4 header word.
pub fn parse_pm4_header(header: u32) -> Pm4Header {
    let packet_type = match (header >> 30) & 0x3 {
        0 => Pm4Type::Type0,
        2 => Pm4Type::Type2,
        3 => Pm4Type::Type3,
        _ => Pm4Type::Type2, // treat unknown as NOP
    };

    let count = ((header >> 16) & 0x3FFF) as u16;
    let opcode = if packet_type == Pm4Type::Type3 {
        Pm4Opcode::from_u32((header >> 8) & 0xFF)
    } else {
        None
    };

    Pm4Header {
        packet_type,
        count,
        opcode,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_nop() {
        // Type 3, Opcode 0x10 (NOP), count=0
        let header: u32 = 0xC000_1000;
        let parsed = parse_pm4_header(header);
        assert_eq!(parsed.packet_type, Pm4Type::Type3);
        assert_eq!(parsed.opcode, Some(Pm4Opcode::Nop));
    }
}
