use crate::cartridge::Cartridge;
use crate::trace::{trace, trace_enabled};

const VRAM_SIZE: usize = 0x2000;
const WRAM_SIZE: usize = 0x2000;
const OAM_SIZE: usize = 0x00A0;
const IO_SIZE: usize = 0x0080;
const HRAM_SIZE: usize = 0x007F;

#[derive(Debug, Clone)]
pub struct Bus {
    cartridge: Cartridge,
    vram: [u8; VRAM_SIZE],
    wram: [u8; WRAM_SIZE],
    oam: [u8; OAM_SIZE],
    io: [u8; IO_SIZE],
    hram: [u8; HRAM_SIZE],
    ie: u8,
    button_state: u8,
}

impl Bus {
    pub fn new(cartridge: Cartridge) -> Self {
        let mut io = [0; IO_SIZE];
        io[0x40] = 0x91; // LCDC: LCD enabled, BG enabled, OBJ enabled, tile data 8000, BG map 9800
        io[0x42] = 0x00; // SCY
        io[0x43] = 0x00; // SCX
        io[0x44] = 0x00; // LY
        io[0x47] = 0xFC; // BGP
        io[0x48] = 0xFF; // OBP0
        io[0x49] = 0xFF; // OBP1
        io[0x4A] = 0x00; // WY
        io[0x4B] = 0x00; // WX

        Self {
            cartridge,
            vram: [0; VRAM_SIZE],
            wram: [0; WRAM_SIZE],
            oam: [0; OAM_SIZE],
            io,
            hram: [0; HRAM_SIZE],
            ie: 0,
            button_state: 0xFF, // All buttons released
        }
    }

    pub fn cartridge(&self) -> &Cartridge {
        &self.cartridge
    }

    pub fn set_button_state(&mut self, buttons: u8) {
        self.button_state = buttons;
    }

    pub fn read8(&self, address: u16) -> u8 {
        match address {
            0x0000..=0x7FFF => self.cartridge.read_rom(address),
            0x8000..=0x9FFF => self.vram[usize::from(address - 0x8000)],
            0xA000..=0xBFFF => self.cartridge.read_ram(address),
            0xC000..=0xDFFF => {
                let value = self.wram[usize::from(address - 0xC000)];
                if trace_enabled() && address == 0xCFC7 {
                    trace(&format!("WRAM read: 0xCFC7 = 0x{:02X}", value));
                }
                value
            }
            0xE000..=0xFDFF => self.wram[usize::from(address - 0xE000)],
            0xFE00..=0xFE9F => self.oam[usize::from(address - 0xFE00)],
            0xFEA0..=0xFEFF => 0xFF,
            0xFF00 => self.read_joypad(),
            0xFF01..=0xFF7F => {
                let value = self.io[usize::from(address - 0xFF00)];
                if trace_enabled() && (address == 0xFF41 || address == 0xFF0F || address == 0xFFFF) {
                    trace(&format!("IO read: 0x{:04X} = 0x{:02X}", address, value));
                }
                value
            }
            0xFF80..=0xFFFE => self.hram[usize::from(address - 0xFF80)],
            0xFFFF => self.ie,
        }
    }

    pub fn write8(&mut self, address: u16, value: u8) {
        match address {
            0x0000..=0x7FFF => self.cartridge.write_rom(address, value),
            0x8000..=0x9FFF => {
                if trace_enabled() && address >= 0x9800 && address <= 0x9BFF {
                    trace(&format!("VRAM BG map write: 0x{:04X} <= 0x{:02X}", address, value));
                }
                self.vram[usize::from(address - 0x8000)] = value;
            }
            0xA000..=0xBFFF => self.cartridge.write_ram(address, value),
            0xC000..=0xDFFF => {
                if trace_enabled() && address == 0xCFC7 {
                    trace(&format!("WRAM write: 0xCFC7 <= 0x{:02X}", value));
                }
                self.wram[usize::from(address - 0xC000)] = value;
            }
            0xE000..=0xFDFF => self.wram[usize::from(address - 0xE000)] = value,
            0xFE00..=0xFE9F => self.oam[usize::from(address - 0xFE00)] = value,
            0xFEA0..=0xFEFF => {},
            0xFF00 => {
                if trace_enabled() {
                    trace(&format!("P1 write: 0xFF00 <= 0x{value:02X}"));
                }
                self.io[0] = value;
            }
            0xFF01..=0xFF7F => {
                let io_index = usize::from(address - 0xFF00);
                if address == 0xFF46 {
                    // OAM DMA transfer from memory page value * 0x100
                    let source = u16::from(value) << 8;
                    for offset in 0..OAM_SIZE {
                        self.oam[offset] = self.read8(source + offset as u16);
                    }
                }
                if trace_enabled() && address == 0xFF47 {
                    trace(&format!("BGP write: 0xFF47 <= 0x{value:02X}"));
                }
                self.io[io_index] = value;
            }
            0xFF80..=0xFFFE => self.hram[usize::from(address - 0xFF80)] = value,
            0xFFFF => self.ie = value,
        }
    }

    fn read_joypad(&self) -> u8 {
        let select = self.io[0];
        let p14 = select & 0x20 == 0; // Action buttons
        let p15 = select & 0x10 == 0; // Direction buttons

        let mut result = 0xF0 | (select & 0x30); // Bits 4-7 high, preserve select bits

        if p14 {
            // Action buttons: A, B, Select, Start (bits 0-3)
            let buttons = self.button_state & 0x0F;
            result |= !buttons & 0x0F; // Active low
        }
        if p15 {
            // Direction buttons: Right, Left, Up, Down (bits 4-7 of button_state -> bits 0-3 of result)
            let buttons = (self.button_state >> 4) & 0x0F;
            result |= !buttons & 0x0F; // Active low
        }

        if trace_enabled() {
            trace(&format!(
                "P1 read: select=0x{select:02X} button_state=0x{button_state:02X} result=0x{result:02X}",
                select = select,
                button_state = self.button_state,
                result = result,
            ));
        }

        result
    }
}
