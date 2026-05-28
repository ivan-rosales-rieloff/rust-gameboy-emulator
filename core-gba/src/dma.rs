//!
//! # GBA Direct Memory Access (DMA) Controller Data Structures
//!
//! This module defines the structures for the 4 DMA channels.
//!

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DmaTrigger {
    Immediate = 0,
    VBlank = 1,
    HBlank = 2,
    Special = 3,
}

#[derive(Debug, Clone, Default)]
pub struct DmaChannel {
    /// Internal registers for address tracking
    pub source_addr: u32,
    pub dest_addr: u32,
    
    /// Latched address registers
    pub current_source: u32,
    pub current_dest: u32,

    /// Shadow copies for repeating
    pub shadow_source: u32,
    pub shadow_dest: u32,

    /// Transfer count
    pub count: u32,
    pub shadow_count: u32,

    /// Control register
    pub control: u16,
    
    /// Is this DMA channel active
    pub enabled: bool,
}

#[derive(Debug, Clone, Default)]
pub struct DmaController {
    pub channels: [DmaChannel; 4],
}

impl DmaController {
    pub fn new() -> Self {
        Self::default()
    }
}
