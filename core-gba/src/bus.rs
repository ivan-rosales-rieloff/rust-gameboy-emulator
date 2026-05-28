//!
//! # GBA Memory Bus
//!
//! This module implements the 32-bit GBA memory address routing, cycle calculations,
//! and I/O register intercepts for PPU, Timers, DMA, and Keypad.
//!

use crate::ppu::Ppu;
use crate::timer::Timers;
use crate::dma::{DmaController, DmaTrigger};

/// Represents the width of a memory access (Byte, Halfword, Word).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessWidth {
    Byte,
    Halfword,
    Word,
}

/// The GBA Memory Bus coordinating memory map translation and waitstates.
#[derive(Debug)]
pub struct Bus {
    /// 16KB System BIOS ROM
    pub bios: Vec<u8>,
    /// 256KB External Work RAM (EWRAM)
    pub ewram: Vec<u8>,
    /// 32KB Internal Work RAM (IWRAM)
    pub iwram: Vec<u8>,
    /// 1KB I/O Registers
    pub io: [u8; 0x400],
    /// 1KB Palette RAM
    pub palette_ram: [u8; 0x400],
    /// 96KB Video RAM (VRAM)
    pub vram: Vec<u8>,
    /// 1KB Object Attribute Memory (OAM)
    pub oam: [u8; 0x400],
    /// Game Pak ROM (Variable size)
    pub rom: Vec<u8>,
    /// 64KB Game Pak Backup SRAM/Flash
    pub sram: Vec<u8>,
    /// Total cycles elapsed during memory operations
    pub cycles: u64,

    /// Live PPU graphics processor
    pub ppu: Ppu,
    /// Live Hardware Timers
    pub timers: Timers,
    /// Live DMA Controller
    pub dma: DmaController,
    /// Keypad Input state (active low: 0 = pressed, 1 = released, default = 0x03FF)
    pub keyinput: u16,
}

impl Default for Bus {
    fn default() -> Self {
        let mut bus = Self {
            bios: vec![0; 0x4000],
            ewram: vec![0; 0x40000],
            iwram: vec![0; 0x8000],
            io: [0; 0x400],
            palette_ram: [0; 0x400],
            vram: vec![0; 0x18000],
            oam: [0; 0x400],
            rom: Vec::new(),
            sram: vec![0; 0x10000],
            cycles: 0,
            ppu: Ppu::new(),
            timers: Timers::new(),
            dma: DmaController::new(),
            keyinput: 0x03FF, // Default all keys released
        };

        // Initialize keyinput registers inside io array as well
        bus.io[0x130] = 0xFF;
        bus.io[0x131] = 0x03;
        bus
    }
}

impl Bus {
    /// Creates a new memory bus with loaded ROM bytes.
    pub fn new(rom: Vec<u8>) -> Self {
        let mut bus = Self::default();
        bus.rom = rom;
        bus
    }

    /// Injects CPU waitstate delays based on memory region and sequential access properties.
    pub fn add_cycles(&mut self, address: u32, width: AccessWidth, seq: bool) {
        let page = (address >> 24) & 0x0F;
        let cycles = match page {
            0x00 => 1, // BIOS: fast 1 cycle access
            0x02 => {
                // EWRAM: 3 cycles for 8/16-bit, 6 cycles for 32-bit (16-bit bus)
                match width {
                    AccessWidth::Byte | AccessWidth::Halfword => 3,
                    AccessWidth::Word => 6,
                }
            }
            0x03 => 1, // IWRAM: fast 32-bit, 1 cycle access
            0x04 => 1, // I/O Registers: 1 cycle
            0x05 => {
                // Palette RAM: 1 cycle for 16-bit, 2 cycles for 32-bit (16-bit bus)
                match width {
                    AccessWidth::Byte | AccessWidth::Halfword => 1,
                    AccessWidth::Word => 2,
                }
            }
            0x06 => {
                // VRAM: 1 cycle for 16-bit, 2 cycles for 32-bit (16-bit bus)
                match width {
                    AccessWidth::Byte | AccessWidth::Halfword => 1,
                    AccessWidth::Word => 2,
                }
            }
            0x07 => 1, // OAM: 1 cycle
            0x08..=0x0D => {
                // Game Pak ROM Waitstates.
                let base = if seq { 2 } else { 5 };
                match width {
                    AccessWidth::Byte | AccessWidth::Halfword => base,
                    AccessWidth::Word => base + 2,
                }
            }
            0x0E => 5, // SRAM: Slow 8-bit, 5 cycles
            _ => 1,
        };
        self.cycles += cycles;
    }

    /// Ticks the PPU and Timers, and dispatches DMA channels if triggered.
    pub fn tick(&mut self, cycles: u32) {
        // Tick timers
        self.timers.tick(cycles, &mut self.io);

        // Tick PPU
        if let Some(trigger) = self.ppu.tick(cycles, &mut self.io, &self.palette_ram, &self.vram, &self.oam) {
            self.trigger_dma(trigger);
        }
    }

    /// Updates the keypad input state.
    pub fn set_button_state(&mut self, buttons: u8) {
        // Map GBA buttons: A, B, Select, Start, Right, Left, Up, Down.
        // GBA keyinput is 10-bit active low.
        let mut key = 0x03FF;
        for i in 0..8 {
            if (buttons & (1 << i)) != 0 {
                key &= !(1 << i);
            }
        }
        self.keyinput = key;

        // Sync with I/O space
        self.io[0x130] = self.keyinput as u8;
        self.io[0x131] = (self.keyinput >> 8) as u8;
    }

    /// Triggers any DMA channels enabled for the given trigger condition.
    pub fn trigger_dma(&mut self, trigger: DmaTrigger) {
        let mut triggered = [false; 4];
        for i in 0..4 {
            let channel = &self.dma.channels[i];
            if channel.enabled {
                let start_timing = (channel.control >> 12) & 3;
                if start_timing as u32 == trigger as u32 {
                    triggered[i] = true;
                }
            }
        }

        for i in 0..4 {
            if triggered[i] {
                self.execute_dma_transfer(i);
            }
        }
    }

    /// Executes the active memory transfer for a DMA channel.
    pub fn execute_dma_transfer(&mut self, channel_idx: usize) {
        // Read everything we need up-front by copying it to local variables to satisfy the borrow checker.
        let (control, mut src, mut dest, count, shadow_count, shadow_dest) = {
            let channel = &self.dma.channels[channel_idx];
            let count = if channel.count == 0 {
                if channel_idx == 3 { 65536 } else { 16384 }
            } else {
                channel.count
            };
            (channel.control, channel.current_source, channel.current_dest, count, channel.shadow_count, channel.shadow_dest)
        };

        let is_32bit = (control & (1 << 10)) != 0;
        let step = if is_32bit { 4 } else { 2 };
        let width = if is_32bit { AccessWidth::Word } else { AccessWidth::Halfword };

        let src_control = (control >> 7) & 3;
        let dest_control = (control >> 5) & 3;

        for _ in 0..count {
            let val = if is_32bit {
                self.read_word(src)
            } else {
                self.read_halfword(src) as u32
            };

            if is_32bit {
                self.write_word(dest, val);
            } else {
                self.write_halfword(dest, val as u16);
            }

            self.add_cycles(src, width, true);

            match src_control {
                0 => src = src.wrapping_add(step),
                1 => src = src.wrapping_sub(step),
                _ => {}
            }

            match dest_control {
                0 | 3 => dest = dest.wrapping_add(step),
                1 => dest = dest.wrapping_sub(step),
                _ => {}
            }
        }

        // Trigger DMA Interrupt if enabled (bit 14 in DMAxCNT_H)
        if (control & (1 << 14)) != 0 {
            let interrupt_bit = 8 + channel_idx as u8;
            let current_if = self.read_halfword(0x04000202);
            self.write_halfword(0x04000202, current_if | (1 << interrupt_bit));
        }

        // Re-borrow channel to update active pointers
        let channel = &mut self.dma.channels[channel_idx];
        channel.current_source = src;
        channel.current_dest = dest;

        let repeat = (control & (1 << 9)) != 0;
        let start_timing = (control >> 12) & 3;

        if repeat && start_timing != 0 {
            channel.count = shadow_count;
            if dest_control == 3 {
                channel.current_dest = shadow_dest;
            }
        } else {
            channel.control &= !(1 << 15);
            channel.enabled = false;
        }
    }

    /// Reads an 8-bit byte from memory.
    pub fn read_byte(&mut self, address: u32) -> u8 {
        let page = (address >> 24) & 0x0F;
        let offset = address & 0x00FFFFFF;

        match page {
            0x00 => {
                if offset < 0x4000 {
                    self.bios[offset as usize]
                } else {
                    0
                }
            }
            0x02 => self.ewram[(offset % 0x40000) as usize],
            0x03 => self.iwram[(offset % 0x8000) as usize],
            0x04 => {
                if offset < 0x400 {
                    // Intercept I/O reads for Timers, DMA, and Keypad
                    if offset >= 0x100 && offset <= 0x10F {
                        let timer_idx = (offset - 0x100) / 4;
                        let reg_idx = (offset - 0x100) % 4;
                        let timer = &self.timers.timers[timer_idx as usize];
                        match reg_idx {
                            0 => timer.counter as u8,
                            1 => (timer.counter >> 8) as u8,
                            2 => timer.control as u8,
                            3 => (timer.control >> 8) as u8,
                            _ => 0,
                        }
                    } else if offset >= 0xB0 && offset <= 0xDF {
                        let channel_idx = (offset - 0xB0) / 12;
                        let reg_idx = (offset - 0xB0) % 12;
                        let channel = &self.dma.channels[channel_idx as usize];
                        match reg_idx {
                            10 => channel.control as u8,
                            11 => (channel.control >> 8) as u8,
                            _ => self.io[offset as usize],
                        }
                    } else if offset == 0x130 {
                        self.keyinput as u8
                    } else if offset == 0x131 {
                        (self.keyinput >> 8) as u8
                    } else {
                        self.io[offset as usize]
                    }
                } else {
                    0
                }
            }
            0x05 => self.palette_ram[(offset % 0x400) as usize],
            0x06 => {
                let mut vram_addr = offset % 0x20000;
                if vram_addr >= 0x18000 {
                    vram_addr -= 0x8000;
                }
                self.vram[vram_addr as usize]
            }
            0x07 => self.oam[(offset % 0x400) as usize],
            0x08..=0x0D => {
                let rom_offset = (address & 0x01FFFFFF) as usize;
                if rom_offset < self.rom.len() {
                    self.rom[rom_offset]
                } else {
                    0
                }
            }
            0x0E => self.sram[(offset % 0x10000) as usize],
            _ => 0,
        }
    }

    /// Reads a 16-bit halfword (aligned).
    pub fn read_halfword(&mut self, address: u32) -> u16 {
        let addr = address & !1;
        let b0 = self.read_byte(addr) as u16;
        let b1 = self.read_byte(addr + 1) as u16;
        b0 | (b1 << 8)
    }

    /// Reads a 32-bit word (aligned).
    pub fn read_word(&mut self, address: u32) -> u32 {
        let addr = address & !3;
        let b0 = self.read_byte(addr) as u32;
        let b1 = self.read_byte(addr + 1) as u32;
        let b2 = self.read_byte(addr + 2) as u32;
        let b3 = self.read_byte(addr + 3) as u32;
        b0 | (b1 << 8) | (b2 << 16) | (b3 << 24)
    }

    /// Writes an 8-bit byte to memory.
    pub fn write_byte(&mut self, address: u32, val: u8) {
        let page = (address >> 24) & 0x0F;
        let offset = address & 0x00FFFFFF;

        match page {
            0x02 => self.ewram[(offset % 0x40000) as usize] = val,
            0x03 => self.iwram[(offset % 0x8000) as usize] = val,
            0x04 => {
                if offset < 0x400 {
                    // Intercept I/O writes for Timers and DMA
                    if offset >= 0x100 && offset <= 0x10F {
                        let timer_idx = ((offset - 0x100) / 4) as usize;
                        let reg_idx = (offset - 0x100) % 4;
                        let timer = &mut self.timers.timers[timer_idx];
                        match reg_idx {
                            0 => timer.reload = (timer.reload & 0xFF00) | val as u16,
                            1 => timer.reload = (timer.reload & 0x00FF) | ((val as u16) << 8),
                            2 => {
                                let old_control = timer.control;
                                timer.control = (old_control & 0xFF00) | val as u16;
                            }
                            3 => {
                                let old_control = timer.control;
                                let new_control = (old_control & 0x00FF) | ((val as u16) << 8);
                                timer.control = new_control;
                                if (new_control & (1 << 7)) != 0 && (old_control & (1 << 7)) == 0 {
                                    timer.active = false;
                                }
                            }
                            _ => {}
                        }
                    } else if offset >= 0xB0 && offset <= 0xDF {
                        let channel_idx = ((offset - 0xB0) / 12) as usize;
                        let reg_idx = (offset - 0xB0) % 12;
                        let channel = &mut self.dma.channels[channel_idx];
                        match reg_idx {
                            0 => channel.source_addr = (channel.source_addr & 0xFFFFFF00) | val as u32,
                            1 => channel.source_addr = (channel.source_addr & 0xFFFF00FF) | ((val as u32) << 8),
                            2 => channel.source_addr = (channel.source_addr & 0xFF00FFFF) | ((val as u32) << 16),
                            3 => channel.source_addr = (channel.source_addr & 0x00FFFFFF) | ((val as u32) << 24),
                            
                            4 => channel.dest_addr = (channel.dest_addr & 0xFFFFFF00) | val as u32,
                            5 => channel.dest_addr = (channel.dest_addr & 0xFFFF00FF) | ((val as u32) << 8),
                            6 => channel.dest_addr = (channel.dest_addr & 0xFF00FFFF) | ((val as u32) << 16),
                            7 => channel.dest_addr = (channel.dest_addr & 0x00FFFFFF) | ((val as u32) << 24),
                            
                            8 => channel.count = (channel.count & 0xFF00) | val as u32,
                            9 => channel.count = (channel.count & 0x00FF) | ((val as u32) << 8),
                            
                            10 => channel.control = (channel.control & 0xFF00) | val as u16,
                            11 => {
                                let old_control = channel.control;
                                let new_control = (old_control & 0x00FF) | ((val as u16) << 8);
                                channel.control = new_control;
                                if (new_control & (1 << 15)) != 0 && (old_control & (1 << 15)) == 0 {
                                    channel.current_source = channel.source_addr;
                                    channel.current_dest = channel.dest_addr;
                                    channel.shadow_source = channel.source_addr;
                                    channel.shadow_dest = channel.dest_addr;
                                    channel.shadow_count = channel.count;
                                    channel.enabled = true;
                                }
                            }
                            _ => {}
                        }
                    } else {
                        self.io[offset as usize] = val;
                    }
                }
            }
            0x05 => self.palette_ram[(offset % 0x400) as usize] = val,
            0x06 => {
                let mut vram_addr = offset % 0x20000;
                if vram_addr >= 0x18000 {
                    vram_addr -= 0x8000;
                }
                self.vram[vram_addr as usize] = val;
            }
            0x07 => self.oam[(offset % 0x400) as usize] = val,
            0x0E => self.sram[(offset % 0x10000) as usize] = val,
            _ => {} // BIOS/ROM are read-only
        }
    }

    /// Writes a 16-bit halfword (aligned).
    pub fn write_halfword(&mut self, address: u32, val: u16) {
        let addr = address & !1;
        self.write_byte(addr, val as u8);
        self.write_byte(addr + 1, (val >> 8) as u8);

        if (address >> 24) & 0x0F == 0x04 {
            let offset = address & 0x00FFFFFF;
            if offset == 0xBA || offset == 0xC6 || offset == 0xD2 || offset == 0xDE {
                let channel_idx = (offset - 0xBA) / 12;
                let control = self.dma.channels[channel_idx as usize].control;
                let enabled = (control & (1 << 15)) != 0;
                let start_timing = (control >> 12) & 3;
                if enabled && start_timing == 0 {
                    self.execute_dma_transfer(channel_idx as usize);
                }
            }
        }
    }

    /// Writes a 32-bit word (aligned).
    pub fn write_word(&mut self, address: u32, val: u32) {
        let addr = address & !3;
        self.write_byte(addr, val as u8);
        self.write_byte(addr + 1, (val >> 8) as u8);
        self.write_byte(addr + 2, (val >> 16) as u8);
        self.write_byte(addr + 3, (val >> 24) as u8);

        if (address >> 24) & 0x0F == 0x04 {
            let offset = address & 0x00FFFFFF;
            if offset == 0xB8 || offset == 0xC4 || offset == 0xD0 || offset == 0xDC {
                let channel_idx = (offset - 0xB8) / 12;
                let control = self.dma.channels[channel_idx as usize].control;
                let enabled = (control & (1 << 15)) != 0;
                let start_timing = (control >> 12) & 3;
                if enabled && start_timing == 0 {
                    self.execute_dma_transfer(channel_idx as usize);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bus_read_write_ram() {
        let mut bus = Bus::default();
        
        // EWRAM
        bus.write_byte(0x02000100, 0xAB);
        assert_eq!(bus.read_byte(0x02000100), 0xAB);

        // IWRAM
        bus.write_halfword(0x03000200, 0x1234);
        assert_eq!(bus.read_halfword(0x03000200), 0x1234);

        bus.write_word(0x03000400, 0xDEADBEEF);
        assert_eq!(bus.read_word(0x03000400), 0xDEADBEEF);
    }

    #[test]
    fn test_bus_waitstate_cycles() {
        let mut bus = Bus::default();
        
        bus.cycles = 0;
        bus.add_cycles(0x03000000, AccessWidth::Word, false);
        assert_eq!(bus.cycles, 1);

        bus.cycles = 0;
        bus.add_cycles(0x02000000, AccessWidth::Word, false);
        assert_eq!(bus.cycles, 6);
    }
}
