use std::error::Error;
use std::fmt::{Display, Formatter};

const MIN_ROM_SIZE: usize = 0x8000;
const TITLE_START: usize = 0x0134;
const TITLE_END: usize = 0x0143;
const CARTRIDGE_TYPE_ADDRESS: usize = 0x0147;
const RAM_BANK_SIZE: usize = 0x2000;
const MAX_MBC1_RAM_BANKS: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CartridgeKind {
    RomOnly,
    Mbc1,
}

#[derive(Debug, Clone)]
pub struct Cartridge {
    rom: Vec<u8>,
    title: String,
    kind: CartridgeKind,
    ram: Vec<u8>,
    rom_bank: u8,
    ram_bank: u8,
    banking_mode: u8,
    ram_enabled: bool,
}

impl Cartridge {
    pub fn from_rom(rom: Vec<u8>) -> Result<Self, CartridgeError> {
        if rom.len() < MIN_ROM_SIZE {
            return Err(CartridgeError::RomTooSmall {
                found: rom.len(),
                minimum: MIN_ROM_SIZE,
            });
        }

        let cartridge_type = rom[CARTRIDGE_TYPE_ADDRESS];
        let kind = match cartridge_type {
            0x00 => CartridgeKind::RomOnly,
            0x01 | 0x02 | 0x03 => CartridgeKind::Mbc1,
            _ => return Err(CartridgeError::UnsupportedCartridgeType(cartridge_type)),
        };

        let title_slice = &rom[TITLE_START..=TITLE_END];
        let title_end = title_slice
            .iter()
            .position(|byte| *byte == 0)
            .unwrap_or(title_slice.len());
        let parsed_title = String::from_utf8_lossy(&title_slice[..title_end]).trim().to_string();
        let title = if parsed_title.is_empty() {
            "UNKNOWN".to_string()
        } else {
            parsed_title
        };

        let ram_size = match kind {
            CartridgeKind::RomOnly => 0,
            CartridgeKind::Mbc1 => RAM_BANK_SIZE * MAX_MBC1_RAM_BANKS,
        };

        Ok(Self {
            rom,
            title,
            kind,
            ram: vec![0; ram_size],
            rom_bank: 1,
            ram_bank: 0,
            banking_mode: 0,
            ram_enabled: false,
        })
    }

    fn current_rom_bank(&self) -> u8 {
        match self.kind {
            CartridgeKind::RomOnly => 1,
            CartridgeKind::Mbc1 => {
                let mut bank = self.rom_bank & 0x1F;
                if bank == 0 {
                    bank = 1;
                }

                if self.banking_mode == 0 {
                    bank |= (self.ram_bank & 0x03) << 5;
                }

                bank
            }
        }
    }

    fn current_ram_bank(&self) -> u8 {
        match self.kind {
            CartridgeKind::RomOnly => 0,
            CartridgeKind::Mbc1 => {
                if self.banking_mode == 0 {
                    0
                } else {
                    self.ram_bank & 0x03
                }
            }
        }
    }

    pub fn read_rom(&self, address: u16) -> u8 {
        match self.kind {
            CartridgeKind::RomOnly => self.rom.get(usize::from(address)).copied().unwrap_or(0xFF),
            CartridgeKind::Mbc1 => match address {
                0x0000..=0x3FFF => self.rom.get(usize::from(address)).copied().unwrap_or(0xFF),
                0x4000..=0x7FFF => {
                    let bank = self.current_rom_bank();
                    let rom_address = usize::from(bank) * 0x4000 + usize::from(address - 0x4000);
                    self.rom.get(rom_address).copied().unwrap_or(0xFF)
                }
                _ => 0xFF,
            },
        }
    }

    pub fn write_rom(&mut self, address: u16, value: u8) {
        if let CartridgeKind::Mbc1 = self.kind {
            match address {
                0x0000..=0x1FFF => {
                    self.ram_enabled = (value & 0x0F) == 0x0A;
                }
                0x2000..=0x3FFF => {
                    let bank = value & 0x1F;
                    self.rom_bank = if bank == 0 { 1 } else { bank };
                }
                0x4000..=0x5FFF => {
                    self.ram_bank = value & 0x03;
                }
                0x6000..=0x7FFF => {
                    self.banking_mode = value & 0x01;
                }
                _ => {}
            }
        }
    }

    pub fn read_ram(&self, address: u16) -> u8 {
        if let CartridgeKind::Mbc1 = self.kind {
            let bank = self.current_ram_bank();
            let offset = usize::from(bank) * RAM_BANK_SIZE + usize::from(address - 0xA000);
            self.ram.get(offset).copied().unwrap_or(0xFF)
        } else {
            0xFF
        }
    }

    pub fn write_ram(&mut self, address: u16, value: u8) {
        if let CartridgeKind::Mbc1 = self.kind {
            if !self.ram_enabled {
                return;
            }

            let bank = self.current_ram_bank();
            let offset = usize::from(bank) * RAM_BANK_SIZE + usize::from(address - 0xA000);
            if let Some(slot) = self.ram.get_mut(offset) {
                *slot = value;
            }
        }
    }

    pub fn title(&self) -> &str {
        &self.title
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CartridgeError {
    RomTooSmall { found: usize, minimum: usize },
    UnsupportedCartridgeType(u8),
}

impl Display for CartridgeError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::RomTooSmall { found, minimum } => {
                write!(
                    f,
                    "ROM too small: found {found} bytes, expected at least {minimum} bytes"
                )
            }
            Self::UnsupportedCartridgeType(value) => {
                write!(
                    f,
                    "unsupported cartridge type 0x{value:02X} (ROM-only and MBC1 are supported)"
                )
            }
        }
    }
}

impl Error for CartridgeError {}
