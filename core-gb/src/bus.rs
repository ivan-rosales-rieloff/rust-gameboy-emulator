use crate::cartridge::Cartridge;

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
        Self {
            cartridge,
            vram: [0; VRAM_SIZE],
            wram: [0; WRAM_SIZE],
            oam: [0; OAM_SIZE],
            io: [0; IO_SIZE],
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
            0xC000..=0xDFFF => self.wram[usize::from(address - 0xC000)],
            0xE000..=0xFDFF => self.wram[usize::from(address - 0xE000)],
            0xFE00..=0xFE9F => self.oam[usize::from(address - 0xFE00)],
            0xFEA0..=0xFEFF => 0xFF,
            0xFF00 => self.read_joypad(),
            0xFF01..=0xFF7F => self.io[usize::from(address - 0xFF00)],
            0xFF80..=0xFFFE => self.hram[usize::from(address - 0xFF80)],
            0xFFFF => self.ie,
        }
    }

    pub fn write8(&mut self, address: u16, value: u8) {
        match address {
            0x0000..=0x7FFF => self.cartridge.write_rom(address, value),
            0x8000..=0x9FFF => self.vram[usize::from(address - 0x8000)] = value,
            0xA000..=0xBFFF => self.cartridge.write_ram(address, value),
            0xC000..=0xDFFF => self.wram[usize::from(address - 0xC000)] = value,
            0xE000..=0xFDFF => self.wram[usize::from(address - 0xE000)] = value,
            0xFE00..=0xFE9F => self.oam[usize::from(address - 0xFE00)] = value,
            0xFEA0..=0xFEFF => {},
            0xFF00 => self.io[0] = value,
            0xFF01..=0xFF7F => self.io[usize::from(address - 0xFF00)] = value,
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

        result
    }
}
