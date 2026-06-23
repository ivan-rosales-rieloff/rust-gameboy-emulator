//!
//! # Game Boy Cartridge System
//!
//! This module implements Game Boy cartridge emulation, including ROM banking,
//! RAM banking, and battery-backed save functionality. The Game Boy uses
//! various Memory Bank Controllers (MBCs) to expand beyond the 32KB ROM limit.
//!
//! ## Cartridge Header (0x0100-0x014F)
//!
//! The cartridge header contains metadata about the game:
//! - **0x0100-0x0103**: Entry point (boot code jumps here)
//! - **0x0104-0x0133**: Nintendo logo (checked by boot ROM)
//! - **0x0134-0x0143**: Game title (11 characters, null-terminated)
//! - **0x0144-0x0145**: Manufacturer code
//! - **0x0146**: CGB flag (Color Game Boy support)
//! - **0x0147**: Cartridge type (MBC type and features)
//! - **0x0148**: ROM size (number of banks)
//! - **0x0149**: RAM size (number of banks)
//! - **0x014A**: Destination code (Japan/overseas)
//!
//! ## Memory Bank Controllers (MBCs)
//!
//! MBCs allow cartridges to have more ROM/RAM than the Game Boy's address space:
//!
//! ### MBC1 (Most Common)
//! - **ROM Banking**: Up to 2MB ROM (128 banks of 16KB)
//! - **RAM Banking**: Up to 32KB RAM (4 banks of 8KB)
//! - **Banking Modes**: Simple (ROM only) or Advanced (ROM+RAM)
//!
//! ### MBC3 (Advanced)
//! - **ROM Banking**: Up to 2MB ROM (128 banks of 16KB)
//! - **RAM Banking**: Up to 32KB RAM (4 banks of 8KB)
//! - **RTC Support**: Real-time clock (syncs with system time)
//! - **Battery Backup**: Save game progress
//!
//! ## Memory Layout
//!
//! The Game Boy maps cartridge memory into its address space:
//! - **0x0000-0x3FFF**: ROM Bank 0 (fixed, 16KB)
//! - **0x4000-0x7FFF**: Switchable ROM Bank (16KB)
//! - **0xA000-0xBFFF**: Switchable RAM Bank (8KB, if present)
//!
//! ## Banking Registers
//!
//! MBCs use writes to ROM addresses to control banking:
//!
//! ### MBC1 Registers
//! - **0x0000-0x1FFF**: RAM Enable (write 0x0A to enable)
//! - **0x2000-0x3FFF**: ROM Bank Number (5 bits)
//! - **0x4000-0x5FFF**: RAM Bank Number / Upper ROM Bank Bits (2 bits)
//! - **0x6000-0x7FFF**: Banking Mode Select (0=ROM, 1=RAM)
//!
//! ### MBC3 Registers
//! - **0x0000-0x1FFF**: RAM/RTC Enable
//! - **0x2000-0x3FFF**: ROM Bank Number (7 bits)
//! - **0x4000-0x5FFF**: RAM Bank Number / RTC Register Select
//! - **0x6000-0x7FFF**: Latch Clock Data
//!
//! ## Battery-Backed RAM
//!
//! Some cartridges include a battery to preserve save data:
//! - **MBC1 with Battery**: Type 0x03
//! - **MBC3 with Battery**: Type 0x13
//! - **Save File**: `{title}.sav` in current directory
//!
//! ## Rust Implementation Notes
//!
//! - Uses `Vec<u8>` for ROM and RAM storage (dynamic sizing)
//! - Pattern matching on cartridge type for MBC-specific behavior
//! - Error handling for unsupported cartridge types and file I/O
//! - Battery save/load happens automatically on cartridge creation/save

use serde::{Deserialize, Serialize};
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::fs;
use std::sync::{Arc, atomic::Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Real-Time Clock state for MBC3
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RtcState {
    pub seconds: u8,
    pub minutes: u8,
    pub hours: u8,
    pub days_low: u8,
    pub days_high: u8,
    pub halt: bool,
    pub carry: bool,
    pub latch_ready: bool,

    // Latched values
    pub latched_seconds: u8,
    pub latched_minutes: u8,
    pub latched_hours: u8,
    pub latched_days_low: u8,
    pub latched_days_high: u8,
    pub latched_halt: bool,
    pub latched_carry: bool,

    pub last_update_time: u64,
}

impl Default for RtcState {
    fn default() -> Self {
        Self::new()
    }
}

impl RtcState {
    pub fn new() -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::default())
            .as_secs();
        Self {
            seconds: 0,
            minutes: 0,
            hours: 0,
            days_low: 0,
            days_high: 0,
            halt: false,
            carry: false,
            latch_ready: false,
            latched_seconds: 0,
            latched_minutes: 0,
            latched_hours: 0,
            latched_days_low: 0,
            latched_days_high: 0,
            latched_halt: false,
            latched_carry: false,
            last_update_time: now,
        }
    }

    pub fn step(&mut self) {
        if self.halt {
            self.last_update_time = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or(Duration::default())
                .as_secs();
            return;
        }

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::default())
            .as_secs();
        if now <= self.last_update_time {
            return;
        }

        let diff = now - self.last_update_time;
        self.last_update_time = now;

        self.add_seconds(diff);
    }

    fn add_seconds(&mut self, secs: u64) {
        let sec_total = self.seconds as u64 + secs;
        self.seconds = (sec_total % 60) as u8;
        let mins = sec_total / 60;

        if mins > 0 {
            let min_total = self.minutes as u64 + mins;
            self.minutes = (min_total % 60) as u8;
            let hrs = min_total / 60;

            if hrs > 0 {
                let hr_total = self.hours as u64 + hrs;
                self.hours = (hr_total % 24) as u8;
                let days = hr_total / 24;

                if days > 0 {
                    let mut current_days = self.days_low as u16 | ((self.days_high as u16) << 8);
                    let new_days = current_days as u64 + days;
                    if new_days > 0x1FF {
                        self.carry = true;
                    }
                    current_days = (new_days % 0x200) as u16;
                    self.days_low = (current_days & 0xFF) as u8;
                    self.days_high = ((current_days >> 8) & 1) as u8;
                }
            }
        }
    }

    pub fn latch(&mut self) {
        self.step();
        self.latched_seconds = self.seconds;
        self.latched_minutes = self.minutes;
        self.latched_hours = self.hours;
        self.latched_days_low = self.days_low;
        self.latched_days_high = self.days_high;
        self.latched_halt = self.halt;
        self.latched_carry = self.carry;
    }

    pub fn read(&self, register: u8) -> u8 {
        match register {
            0x08 => self.latched_seconds,
            0x09 => self.latched_minutes,
            0x0A => self.latched_hours,
            0x0B => self.latched_days_low,
            0x0C => {
                let mut v = 0;
                if self.latched_days_high > 0 {
                    v |= 1;
                }
                if self.latched_halt {
                    v |= 0x40;
                }
                if self.latched_carry {
                    v |= 0x80;
                }
                v
            }
            _ => 0xFF,
        }
    }

    pub fn write(&mut self, register: u8, value: u8) {
        self.step();
        match register {
            0x08 => self.seconds = value % 60,
            0x09 => self.minutes = value % 60,
            0x0A => self.hours = value % 24,
            0x0B => self.days_low = value,
            0x0C => {
                self.days_high = value & 1;
                self.halt = (value & 0x40) != 0;
                self.carry = (value & 0x80) != 0;
            }
            _ => {}
        }
    }
}

/// Minimum valid ROM size (32KB - two 16KB banks)
const MIN_ROM_SIZE: usize = 0x8000;

/// Cartridge header offsets for metadata
const TITLE_START: usize = 0x0134; // Game title start
const TITLE_END: usize = 0x0143; // Game title end
const CARTRIDGE_TYPE_ADDRESS: usize = 0x0147; // MBC type

/// RAM bank size (8KB per bank)
const RAM_BANK_SIZE: usize = 0x2000;

/// Maximum RAM banks for MBC1 (4 banks = 32KB)
const MAX_MBC1_RAM_BANKS: usize = 4;

/// Supported cartridge types with their MBC features.
///
/// The Game Boy supports various MBC chips that provide banking functionality.
/// Each type has different ROM/RAM capacities and features.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
enum CartridgeKind {
    /// No MBC - 32KB ROM only, no RAM
    RomOnly,
    /// MBC1 - ROM banking only
    Mbc1,
    /// MBC1 with RAM (no battery)
    Mbc1Ram,
    /// MBC1 with RAM and battery backup
    Mbc1RamBattery,
    /// MBC2 - ROM banking and built-in RAM (no battery)
    Mbc2,
    /// MBC2 with battery backup
    Mbc2Battery,
    /// MBC3 with RAM (no battery)
    Mbc3Ram,
    /// MBC3 with RAM and battery backup
    Mbc3RamBattery,
    /// MBC5 - ROM banking only
    Mbc5,
    /// MBC5 with RAM (no battery)
    Mbc5Ram,
    /// MBC5 with RAM and battery backup
    Mbc5RamBattery,
    /// MBC5 with Rumble motor
    Mbc5Rumble,
    /// MBC5 with Rumble motor and RAM (no battery)
    Mbc5RumbleRam,
    /// MBC5 with Rumble motor, RAM and battery backup
    Mbc5RumbleRamBattery,
    /// ROM + RAM without MBC
    RomRam,
    /// ROM + RAM with battery backup
    RomRamBattery,
}

/// Game Boy cartridge emulator.
///
/// Handles ROM loading, MBC banking logic, RAM management, and save file I/O.
/// Supports ROM-only, MBC1, MBC3, and MBC5 cartridges with various RAM configurations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Cartridge {
    /// Complete ROM data (may be multiple banks)
    rom: Vec<u8>,

    /// Game title extracted from cartridge header
    title: String,

    /// Type of cartridge/MBC chip
    kind: CartridgeKind,

    /// Cartridge RAM (if present, may be banked)
    ram: Vec<u8>,

    /// Current ROM bank register (affects 0x4000-0x7FFF)
    rom_bank: u16,

    /// Current RAM bank register (affects 0xA000-0xBFFF)
    ram_bank: u8,

    /// MBC1 banking mode (0=ROM banking, 1=RAM banking)
    banking_mode: u8,

    /// RAM enable flag (must be set to access cartridge RAM)
    ram_enabled: bool,

    /// Whether this cartridge supports battery-backed saves
    has_battery: bool,

    /// Whether this cartridge is GBC-compatible (based on 0x0143 byte)
    is_cgb: bool,
    /// MBC3 Real-Time Clock state
    rtc: RtcState,
    /// MBC5 Rumble active state
    pub rumble_active: bool,
    /// Debounce interval for auto-saving battery-backed RAM (skipped for serde)
    #[serde(skip)]
    save_debounce_ms: Duration,
    /// Whether the cartridge RAM has unstaged changes (skipped for serde)
    #[serde(skip)]
    ram_dirty: Arc<std::sync::atomic::AtomicBool>,
    /// Last RAM write timestamp to coordinate debounced updates (skipped for serde)
    #[serde(skip)]
    last_write_time: Arc<std::sync::Mutex<Option<std::time::Instant>>>,
}

impl Cartridge {
    /// Creates a cartridge from ROM data.
    ///
    /// Parses the cartridge header to determine MBC type, allocates RAM if needed,
    /// and loads battery-backed save data if present.
    ///
    /// # Arguments
    /// * `rom` - Complete ROM data as bytes
    ///
    /// # Returns
    /// * `Ok(Cartridge)` - Successfully loaded cartridge
    /// * `Err(CartridgeError)` - Invalid ROM or unsupported cartridge type
    ///
    /// # Supported Cartridge Types
    /// - 0x00: ROM-only
    /// - 0x01: MBC1
    /// - 0x02: MBC1+RAM
    /// - 0x03: MBC1+RAM+Battery
    /// - 0x11: MBC3+RAM
    /// - 0x12: MBC3+RAM
    /// - 0x13: MBC3+RAM+Battery
    /// - 0x19: MBC5
    /// - 0x1A: MBC5+RAM
    /// - 0x1B: MBC5+RAM+Battery
    pub fn from_rom(rom: Vec<u8>) -> Result<Self, CartridgeError> {
        // Validate minimum ROM size
        if rom.len() < MIN_ROM_SIZE {
            return Err(CartridgeError::RomTooSmall {
                found: rom.len(),
                minimum: MIN_ROM_SIZE,
            });
        }

        // Determine cartridge type from header
        let cartridge_type = rom[CARTRIDGE_TYPE_ADDRESS];
        let (kind, has_battery) = match cartridge_type {
            0x00 => (CartridgeKind::RomOnly, false),       // ROM-only
            0x01 => (CartridgeKind::Mbc1, false),          // MBC1
            0x02 => (CartridgeKind::Mbc1Ram, false),       // MBC1+RAM
            0x03 => (CartridgeKind::Mbc1RamBattery, true), // MBC1+RAM+Battery
            0x05 => (CartridgeKind::Mbc2, false),          // MBC2
            0x06 => (CartridgeKind::Mbc2Battery, true),    // MBC2+Battery
            0x08 => (CartridgeKind::RomRam, false),        // ROM+RAM
            0x09 => (CartridgeKind::RomRamBattery, true),  // ROM+RAM+Battery
            0x0F => (CartridgeKind::Mbc3RamBattery, true), // MBC3+TIMER+BATTERY
            0x10 => (CartridgeKind::Mbc3RamBattery, true), // MBC3+TIMER+RAM+BATTERY
            0x11 => (CartridgeKind::Mbc3Ram, false),       // MBC3
            0x12 => (CartridgeKind::Mbc3Ram, false),       // MBC3+RAM
            0x13 => (CartridgeKind::Mbc3RamBattery, true), // MBC3+RAM+Battery
            0x19 => (CartridgeKind::Mbc5, false),          // MBC5
            0x1A => (CartridgeKind::Mbc5Ram, false),       // MBC5+RAM
            0x1B => (CartridgeKind::Mbc5RamBattery, true), // MBC5+RAM+Battery
            0x1C => (CartridgeKind::Mbc5Rumble, false),    // MBC5+RUMBLE
            0x1D => (CartridgeKind::Mbc5RumbleRam, false), // MBC5+RUMBLE+RAM
            0x1E => (CartridgeKind::Mbc5RumbleRamBattery, true), // MBC5+RUMBLE+RAM+BATTERY
            _ => return Err(CartridgeError::UnsupportedCartridgeType(cartridge_type)),
        };

        // Extract game title from header (null-terminated string)
        let title_slice = &rom[TITLE_START..=TITLE_END];
        let title_end = title_slice
            .iter()
            .position(|byte| *byte == 0) // Find null terminator
            .unwrap_or(title_slice.len());
        let parsed_title = String::from_utf8_lossy(&title_slice[..title_end])
            .trim()
            .to_string();
        let title = if parsed_title.is_empty() {
            "UNKNOWN".to_string()
        } else {
            parsed_title
        };

        // Determine RAM size based on cartridge type
        let ram_size = match kind {
            CartridgeKind::RomOnly | CartridgeKind::Mbc1 | CartridgeKind::Mbc5 | CartridgeKind::Mbc5Rumble => 0, // No RAM
            CartridgeKind::Mbc2 | CartridgeKind::Mbc2Battery => 512, // MBC2 built-in 512 half-bytes
            CartridgeKind::RomRam | CartridgeKind::RomRamBattery => {
                let ram_size_code = rom[0x0149];
                match ram_size_code {
                    0x00 => 0,
                    0x01 => 2048,
                    0x02 => 8192,
                    0x03 => 32768,
                    _ => 8192,
                }
            }
            // MBC1/MBC3 with RAM: 4 banks of 8KB = 32KB total (except type 0x0F which has no RAM)
            CartridgeKind::Mbc1Ram
            | CartridgeKind::Mbc1RamBattery
            | CartridgeKind::Mbc3Ram
            | CartridgeKind::Mbc3RamBattery => {
                if cartridge_type == 0x0F {
                    0
                } else {
                    RAM_BANK_SIZE * MAX_MBC1_RAM_BANKS
                }
            }
            CartridgeKind::Mbc5Ram | CartridgeKind::Mbc5RamBattery | CartridgeKind::Mbc5RumbleRam | CartridgeKind::Mbc5RumbleRamBattery => {
                let ram_size_code = rom[0x0149];
                let banks = match ram_size_code {
                    0x00 => 0,
                    0x01 => 1,
                    0x02 => 1,
                    0x03 => 4,
                    0x04 => 16,
                    0x05 => 8,
                    _ => 4, // Fallback to 32KB
                };
                RAM_BANK_SIZE * banks
            }
        };

        // GBC compatibility flag is specified at address 0x0143 in the cartridge header.
        // - Bit 7 set (0x80) indicates GBC support (backward compatible with standard Game Boy).
        // - Value 0xC0 indicates GBC-only (fails to run on standard DMG monochrome systems).
        let cgb_flag = rom[0x0143];
        let is_cgb = (cgb_flag & 0x80) != 0;

        // Create cartridge instance
        let mut cartridge = Self {
            rom,
            title: title.clone(),
            kind,
            ram: vec![0; ram_size], // Initialize RAM to zeros
            rom_bank: 1,            // Default to bank 1 (bank 0 is always accessible)
            ram_bank: 0,            // Default to RAM bank 0
            banking_mode: 0,        // Default to ROM banking mode (MBC1)
            ram_enabled: false,     // RAM starts disabled
            has_battery,
            is_cgb,
            rtc: RtcState::new(),
            rumble_active: false,
            save_debounce_ms: Duration::from_millis(500),
            ram_dirty: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            last_write_time: Arc::new(std::sync::Mutex::new(None)),
        };

        // Load save file if battery-backed RAM is supported
        if has_battery {
            cartridge.load_save_file();
        }

        Ok(cartridge)
    }

    /// Calculates the current ROM bank number based on MBC type and registers.
    ///
    /// Different MBCs have different banking logic and bank number ranges.
    /// Bank 0 is always accessible at 0x0000-0x3FFF, so this returns banks 1+.
    fn current_rom_bank(&self) -> u16 {
        match self.kind {
            CartridgeKind::RomOnly | CartridgeKind::RomRam | CartridgeKind::RomRamBattery => 1, // Fixed bank 1 (effectively bank 0)

            CartridgeKind::Mbc2 | CartridgeKind::Mbc2Battery => {
                let bank = self.rom_bank & 0x0F; // 4 bits
                if bank == 0 { 1 } else { bank }
            }

            CartridgeKind::Mbc1 | CartridgeKind::Mbc1Ram | CartridgeKind::Mbc1RamBattery => {
                let mut bank = self.rom_bank & 0x1F; // Lower 5 bits from register

                // MBC1 quirk: bank 0 maps to bank 1
                if bank == 0 {
                    bank = 1;
                }

                // In ROM banking mode, upper bits come from RAM bank register
                if self.banking_mode == 0 {
                    bank |= (u16::from(self.ram_bank) & 0x03) << 5; // Add upper 2 bits
                }

                bank
            }

            CartridgeKind::Mbc3Ram | CartridgeKind::Mbc3RamBattery => {
                let mut bank = self.rom_bank & 0x7F; // 7 bits (128 banks max)

                // MBC3 quirk: bank 0 maps to bank 1
                if bank == 0 {
                    bank = 1;
                }

                bank
            }

            CartridgeKind::Mbc5 | CartridgeKind::Mbc5Ram | CartridgeKind::Mbc5RamBattery | CartridgeKind::Mbc5Rumble | CartridgeKind::Mbc5RumbleRam | CartridgeKind::Mbc5RumbleRamBattery => {
                self.rom_bank & 0x1FF
            }
        }
    }

    /// Calculates the current RAM bank number based on MBC type and registers.
    ///
    /// RAM banking is simpler than ROM banking - just uses the RAM bank register
    /// with some MBC-specific masking.
    fn current_ram_bank(&self) -> u8 {
        match self.kind {
            CartridgeKind::RomOnly | CartridgeKind::RomRam | CartridgeKind::RomRamBattery | CartridgeKind::Mbc2 | CartridgeKind::Mbc2Battery | CartridgeKind::Mbc5 | CartridgeKind::Mbc5Rumble => 0, // No banking
            CartridgeKind::Mbc1 | CartridgeKind::Mbc1Ram | CartridgeKind::Mbc1RamBattery => {
                if self.banking_mode == 0 {
                    0 // ROM banking mode uses bank 0
                } else {
                    self.ram_bank & 0x03 // RAM banking mode uses register value
                }
            }
            CartridgeKind::Mbc3Ram | CartridgeKind::Mbc3RamBattery => {
                self.ram_bank & 0x03 // Always use lower 2 bits
            }
            CartridgeKind::Mbc5Ram | CartridgeKind::Mbc5RamBattery => {
                self.ram_bank & 0x0F // Mask lower 4 bits (up to 16 RAM banks)
            }
            CartridgeKind::Mbc5RumbleRam | CartridgeKind::Mbc5RumbleRamBattery => {
                self.ram_bank & 0x07 // Mask lower 3 bits (up to 8 RAM banks)
            }
        }
    }

    /// Reads a byte from cartridge ROM.
    ///
    /// Handles banking for the switchable ROM area (0x4000-0x7FFF).
    /// Bank 0 (0x0000-0x3FFF) is always directly accessible.
    ///
    /// # Arguments
    /// * `address` - Memory address to read (0x0000-0x7FFF)
    ///
    /// # Returns
    /// * ROM byte at the specified address
    pub fn read_rom(&self, address: u16) -> u8 {
        match self.kind {
            CartridgeKind::RomOnly | CartridgeKind::RomRam | CartridgeKind::RomRamBattery => {
                // Direct access to ROM (no banking)
                self.rom.get(usize::from(address)).copied().unwrap_or(0xFF)
            }

            CartridgeKind::Mbc1
            | CartridgeKind::Mbc1Ram
            | CartridgeKind::Mbc1RamBattery
            | CartridgeKind::Mbc2
            | CartridgeKind::Mbc2Battery
            | CartridgeKind::Mbc3Ram
            | CartridgeKind::Mbc3RamBattery
            | CartridgeKind::Mbc5
            | CartridgeKind::Mbc5Ram
            | CartridgeKind::Mbc5RamBattery
            | CartridgeKind::Mbc5Rumble
            | CartridgeKind::Mbc5RumbleRam
            | CartridgeKind::Mbc5RumbleRamBattery => {
                match address {
                    0x0000..=0x3FFF => {
                        // Bank 0 - always accessible
                        self.rom.get(usize::from(address)).copied().unwrap_or(0xFF)
                    }
                    0x4000..=0x7FFF => {
                        // Switchable bank - apply banking logic
                        let bank = self.current_rom_bank();
                        let rom_address =
                            usize::from(bank) * 0x4000 + usize::from(address - 0x4000);
                        self.rom.get(rom_address).copied().unwrap_or(0xFF)
                    }
                    _ => 0xFF, // Invalid address range
                }
            }
        }
    }

    /// Writes to cartridge ROM area (actually writes to MBC registers).
    ///
    /// The Game Boy uses writes to ROM addresses to control MBC banking registers.
    /// This doesn't modify the ROM data itself.
    ///
    /// # Arguments
    /// * `address` - Memory address being written to (0x0000-0x7FFF)
    /// * `value` - Value being written
    pub fn write_rom(&mut self, address: u16, value: u8) {
        match self.kind {
            CartridgeKind::RomOnly | CartridgeKind::RomRam | CartridgeKind::RomRamBattery => {
                // ROM-only cartridges ignore writes to ROM area
            }

            CartridgeKind::Mbc2 | CartridgeKind::Mbc2Battery => {
                match address {
                    0x0000..=0x3FFF => {
                        // The least significant bit of the upper address byte controls whether RAM is enabled
                        // or ROM bank is selected.
                        if (address & 0x0100) == 0 {
                            self.ram_enabled = (value & 0x0F) == 0x0A;
                        } else {
                            let bank = value & 0x0F;
                            self.rom_bank = if bank == 0 { 1 } else { u16::from(bank) };
                        }
                    }
                    _ => {} // MBC2 ignores writes to 0x4000-0x7FFF
                }
            }

            CartridgeKind::Mbc1 | CartridgeKind::Mbc1Ram | CartridgeKind::Mbc1RamBattery => {
                match address {
                    0x0000..=0x1FFF => {
                        // RAM enable/disable register
                        // Writing 0x0A enables RAM, any other value disables it
                        self.ram_enabled = (value & 0x0F) == 0x0A;
                    }
                    0x2000..=0x3FFF => {
                        // ROM bank register (lower 5 bits)
                        let bank = value & 0x1F;
                        self.rom_bank = if bank == 0 { 1 } else { u16::from(bank) }; // Bank 0 -> 1
                    }
                    0x4000..=0x5FFF => {
                        // RAM bank register (2 bits) or upper ROM bank bits
                        self.ram_bank = value & 0x03;
                    }
                    0x6000..=0x7FFF => {
                        // Banking mode select (0=ROM banking, 1=RAM banking)
                        self.banking_mode = value & 0x01;
                    }
                    _ => {} // Invalid range
                }
            }

            CartridgeKind::Mbc3Ram | CartridgeKind::Mbc3RamBattery => {
                match address {
                    0x0000..=0x1FFF => {
                        // RAM/RTC enable register
                        self.ram_enabled = (value & 0x0F) == 0x0A;
                    }
                    0x2000..=0x3FFF => {
                        // ROM bank register (7 bits)
                        let bank = value & 0x7F;
                        self.rom_bank = if bank == 0 { 1 } else { u16::from(bank) }; // Bank 0 -> 1
                    }
                    0x4000..=0x5FFF => {
                        // RAM bank register (2 bits) or RTC register select
                        self.ram_bank = value;
                    }
                    0x6000..=0x7FFF => {
                        // Latch clock data
                        if self.banking_mode == 0 && value == 1 {
                            self.rtc.latch();
                        }
                        self.banking_mode = value; // Use banking_mode to store the previous latch state
                    }
                    _ => {} // Invalid range
                }
            }

            CartridgeKind::Mbc5 | CartridgeKind::Mbc5Ram | CartridgeKind::Mbc5RamBattery | CartridgeKind::Mbc5Rumble | CartridgeKind::Mbc5RumbleRam | CartridgeKind::Mbc5RumbleRamBattery => {
                match address {
                    0x0000..=0x1FFF => {
                        // RAM enable/disable register
                        // Writing 0x0A enables RAM, any other value disables it
                        self.ram_enabled = (value & 0x0F) == 0x0A;
                    }
                    0x2000..=0x2FFF => {
                        // ROM bank register (lower 8 bits)
                        self.rom_bank = (self.rom_bank & 0x0100) | u16::from(value);
                    }
                    0x3000..=0x3FFF => {
                        // ROM bank register (9th bit)
                        let bit_9 = u16::from(value & 0x01);
                        self.rom_bank = (self.rom_bank & 0x00FF) | (bit_9 << 8);
                    }
                    0x4000..=0x5FFF => {
                        if matches!(self.kind, CartridgeKind::Mbc5Rumble | CartridgeKind::Mbc5RumbleRam | CartridgeKind::Mbc5RumbleRamBattery) {
                            self.rumble_active = (value & 0x08) != 0;
                            self.ram_bank = value & 0x07;
                        } else {
                            self.ram_bank = value & 0x0F;
                        }
                    }
                    _ => {} // Invalid range
                }
            }
        }
    }

    /// Reads a byte from cartridge RAM.
    ///
    /// RAM access is controlled by the RAM enable flag and current RAM bank.
    /// Returns 0xFF if RAM is disabled or not present.
    ///
    /// # Arguments
    /// * `address` - Memory address to read (0xA000-0xBFFF)
    ///
    /// # Returns
    /// * RAM byte at the specified address, or 0xFF if inaccessible
    pub fn read_ram(&self, address: u16) -> u8 {
        match self.kind {
            CartridgeKind::RomOnly | CartridgeKind::Mbc1 | CartridgeKind::Mbc5 | CartridgeKind::Mbc5Rumble => {
                // No RAM present
                0xFF
            }

            CartridgeKind::RomRam | CartridgeKind::RomRamBattery => {
                let offset = usize::from(address - 0xA000);
                self.ram.get(offset).copied().unwrap_or(0xFF)
            }

            CartridgeKind::Mbc2 | CartridgeKind::Mbc2Battery => {
                if !self.ram_enabled {
                    return 0xFF;
                }
                let offset = usize::from((address - 0xA000) & 0x01FF); // 512 bytes wrap
                self.ram.get(offset).copied().unwrap_or(0xFF) | 0xF0 // Upper 4 bits are 1
            }

            CartridgeKind::Mbc1Ram
            | CartridgeKind::Mbc1RamBattery
            | CartridgeKind::Mbc3Ram
            | CartridgeKind::Mbc3RamBattery
            | CartridgeKind::Mbc5Ram
            | CartridgeKind::Mbc5RamBattery
            | CartridgeKind::Mbc5RumbleRam
            | CartridgeKind::Mbc5RumbleRamBattery => {
                if !self.ram_enabled {
                    return 0xFF; // RAM disabled
                }

                if (self.kind == CartridgeKind::Mbc3Ram || self.kind == CartridgeKind::Mbc3RamBattery)
                    && self.ram_bank >= 0x08
                    && self.ram_bank <= 0x0C
                {
                    return self.rtc.read(self.ram_bank);
                }

                // Calculate RAM address with banking
                let bank = self.current_ram_bank();
                let offset = usize::from(bank) * RAM_BANK_SIZE + usize::from(address - 0xA000);
                self.ram.get(offset).copied().unwrap_or(0xFF)
            }
        }
    }

    /// Writes a byte to cartridge RAM.
    ///
    /// RAM must be enabled and present for the write to succeed.
    /// Battery-backed cartridges will save this data to disk.
    ///
    /// # Arguments
    /// * `address` - Memory address to write (0xA000-0xBFFF)
    /// * `value` - Value to write
    pub fn write_ram(&mut self, address: u16, value: u8) {
        match self.kind {
            CartridgeKind::RomOnly | CartridgeKind::Mbc1 | CartridgeKind::Mbc5 | CartridgeKind::Mbc5Rumble => {
                // No RAM present - ignore write
            }

            CartridgeKind::RomRam | CartridgeKind::RomRamBattery => {
                let offset = usize::from(address - 0xA000);
                if let Some(slot) = self.ram.get_mut(offset) {
                    *slot = value;
                    if self.has_battery {
                        self.ram_dirty.store(true, Ordering::SeqCst);
                        let mut lock = self.last_write_time.lock().unwrap();
                        *lock = Some(std::time::Instant::now());
                    }
                }
            }

            CartridgeKind::Mbc2 | CartridgeKind::Mbc2Battery => {
                if !self.ram_enabled {
                    return;
                }
                let offset = usize::from((address - 0xA000) & 0x01FF); // 512 bytes wrap
                if let Some(slot) = self.ram.get_mut(offset) {
                    *slot = value & 0x0F; // 4-bit masking
                    if self.has_battery {
                        self.ram_dirty.store(true, Ordering::SeqCst);
                        let mut lock = self.last_write_time.lock().unwrap();
                        *lock = Some(std::time::Instant::now());
                    }
                }
            }

            CartridgeKind::Mbc1Ram
            | CartridgeKind::Mbc1RamBattery
            | CartridgeKind::Mbc3Ram
            | CartridgeKind::Mbc3RamBattery
            | CartridgeKind::Mbc5Ram
            | CartridgeKind::Mbc5RamBattery
            | CartridgeKind::Mbc5RumbleRam
            | CartridgeKind::Mbc5RumbleRamBattery => {
                if !self.ram_enabled {
                    return; // RAM disabled - ignore write
                }

                if (self.kind == CartridgeKind::Mbc3Ram || self.kind == CartridgeKind::Mbc3RamBattery)
                    && self.ram_bank >= 0x08
                    && self.ram_bank <= 0x0C
                {
                    self.rtc.write(self.ram_bank, value);
                    return;
                }

                // Calculate RAM address with banking
                let bank = self.current_ram_bank();
                let offset = usize::from(bank) * RAM_BANK_SIZE + usize::from(address - 0xA000);

                // Write to RAM if address is valid
                if let Some(slot) = self.ram.get_mut(offset) {
                    *slot = value;
                    if self.has_battery {
                        self.ram_dirty.store(true, Ordering::SeqCst);
                        let mut lock = self.last_write_time.lock().unwrap();
                        *lock = Some(std::time::Instant::now());
                    }
                }
            }
        }
    }

    /// Checks if a debounced RAM save needs to be written to disk.
    ///
    /// This is called automatically at the end of each frame if the emulator is running.
    pub fn update_save_debouncer(&mut self) {
        if !self.has_battery || self.ram.is_empty() {
            return;
        }

        if self.ram_dirty.load(Ordering::SeqCst) {
            let last_write = {
                let lock = self.last_write_time.lock().unwrap();
                *lock
            };

            if let Some(last_time) = last_write {
                if last_time.elapsed() >= self.save_debounce_ms {
                    // Clear dirty flag and last write time
                    self.ram_dirty.store(false, Ordering::SeqCst);
                    {
                        let mut lock = self.last_write_time.lock().unwrap();
                        *lock = None;
                    }

                    // Clone the current RAM and write in background
                    let title = self.title.clone();
                    let snapshot = self.ram.clone();
                    std::thread::spawn(move || {
                        let _ = Cartridge::persist_snapshot(&title, &snapshot);
                    });
                }
            }
        }
    }

    /// Persist current RAM to the cartridge-state file used by the frontend.
    /// deprecated: use `persist_snapshot` instead with a provided RAM snapshot.
    // fn persist_cartridge_state(&self) -> Result<(), CartridgeError> {
    //     if !self.has_battery || self.ram.is_empty() {
    //         return Ok(());
    //     }

    //     let saves_dir = std::path::Path::new("saves");
    //     std::fs::create_dir_all(saves_dir).map_err(|e| CartridgeError::SaveError {
    //         path: saves_dir.display().to_string(),
    //         error: e.to_string(),
    //     })?;

    //     let state_path = saves_dir.join(format!("{}.catrigestate", self.title));
    //     let p = state_path.display().to_string();
    //     std::fs::write(&state_path, &self.ram).map_err(|e| CartridgeError::SaveError {
    //         path: p,
    //         error: e.to_string(),
    //     })
    // }

    /// Persist a provided RAM snapshot to cartridge-state file (used by debounced save tasks).
    fn persist_snapshot(title: &str, snapshot: &[u8]) -> Result<(), CartridgeError> {
        if snapshot.is_empty() {
            return Ok(());
        }

        let saves_dir = std::path::Path::new("saves");
        std::fs::create_dir_all(saves_dir).map_err(|e| CartridgeError::SaveError {
            path: saves_dir.display().to_string(),
            error: e.to_string(),
        })?;

        let state_path = saves_dir.join(format!("{}.catrigestate", title));
        let p = state_path.display().to_string();
        std::fs::write(&state_path, snapshot).map_err(|e| CartridgeError::SaveError {
            path: p,
            error: e.to_string(),
        })
    }

    /// Returns the game title from the cartridge header.
    pub fn title(&self) -> &str {
        &self.title
    }

    /// Returns whether this cartridge supports battery-backed saves.
    pub fn has_battery(&self) -> bool {
        self.has_battery
    }

    /// Returns whether this cartridge is GBC-compatible.
    pub fn is_cgb(&self) -> bool {
        self.is_cgb
    }

    /// Saves the current RAM contents to a save file.
    ///
    /// Only works for battery-backed cartridges with RAM.
    /// Save file is named `{title}.sav` in the current directory.
    ///
    /// # Returns
    /// * `Ok(())` - Save successful or not needed
    /// * `Err(CartridgeError)` - File I/O error
    pub fn save_game(&self) -> Result<(), CartridgeError> {
        if !self.has_battery || self.ram.is_empty() {
            return Ok(()); // No save needed
        }

        // Default save directory: "saves/" in current working dir
        let saves_dir = std::path::Path::new("saves");
        if let Err(e) = std::fs::create_dir_all(saves_dir) {
            return Err(CartridgeError::SaveError {
                path: saves_dir.display().to_string(),
                error: e.to_string(),
            });
        }

        let save_path = saves_dir.join(format!("{}.sav", self.title));
        let save_path_str = save_path.display().to_string();
        fs::write(&save_path, &self.ram).map_err(|e| CartridgeError::SaveError {
            path: save_path_str,
            error: e.to_string(),
        })
    }

    /// Saves the current RAM contents to the specified path.
    pub fn save_game_to<P: AsRef<std::path::Path>>(&self, path: P) -> Result<(), CartridgeError> {
        if !self.has_battery || self.ram.is_empty() {
            return Ok(());
        }

        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                return Err(CartridgeError::SaveError {
                    path: parent.display().to_string(),
                    error: e.to_string(),
                });
            }
        }

        let save_path_str = path.display().to_string();
        fs::write(path, &self.ram).map_err(|e| CartridgeError::SaveError {
            path: save_path_str,
            error: e.to_string(),
        })
    }

    /// Loads RAM contents from the specified save file path into cartridge RAM.
    /// Returns an error if file cannot be read or size doesn't match.
    pub fn load_game_from<P: AsRef<std::path::Path>>(&mut self, path: P) -> Result<(), CartridgeError> {
        if !self.has_battery || self.ram.is_empty() {
            return Ok(());
        }

        let path = path.as_ref();
        let save_path_str = path.display().to_string();
        let data = std::fs::read(path).map_err(|e| CartridgeError::LoadError {
            path: save_path_str.clone(),
            error: e.to_string(),
        })?;

        if data.len() != self.ram.len() {
            return Err(CartridgeError::LoadError {
                path: save_path_str,
                error: format!("save size {} does not match cartridge RAM size {}", data.len(), self.ram.len()),
            });
        }

        self.ram.copy_from_slice(&data);
        Ok(())
    }

    /// Loads save data from a save file into RAM.
    ///
    /// Called automatically during cartridge initialization for battery-backed cartridges.
    /// Only loads if the save file exists and matches the expected RAM size.
    fn load_save_file(&mut self) {
        if !self.has_battery || self.ram.is_empty() {
            return; // No save file expected
        }

        // Prefer the cartridge-state file used by the frontend for load/save
        let state_path = std::path::Path::new("saves").join(format!("{}.catrigestate", self.title));
        if let Ok(save_data) = fs::read(&state_path) {
            if save_data.len() == self.ram.len() {
                self.ram.copy_from_slice(&save_data);
                return;
            }
        }

        // Fallback to legacy .sav files for compatibility
        let save_path = std::path::Path::new("saves").join(format!("{}.sav", self.title));
        if let Ok(save_data) = fs::read(&save_path) {
            if save_data.len() == self.ram.len() {
                self.ram.copy_from_slice(&save_data);
            }
        }
        // If file doesn't exist or size doesn't match, RAM stays initialized to zeros
    }
}

/// Errors that can occur during cartridge operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CartridgeError {
    /// ROM file is too small to be valid
    RomTooSmall { found: usize, minimum: usize },

    /// Cartridge type is not supported by this emulator
    UnsupportedCartridgeType(u8),

    /// Failed to save game data to file
    SaveError { path: String, error: String },
    /// Failed to load game data from file
    LoadError { path: String, error: String },
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
                    "unsupported cartridge type 0x{value:02X} (ROM-only, MBC1, and MBC3 are supported)"
                )
            }
            Self::SaveError { path, error } => {
                write!(f, "failed to save game to '{}': {}", path, error)
            }
            Self::LoadError { path, error } => {
                write!(f, "failed to load game from '{}': {}", path, error)
            }
        }
    }
}

impl Error for CartridgeError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_rom(cartridge_type: u8, rom_size: usize) -> Vec<u8> {
        let mut rom = vec![0; std::cmp::max(rom_size, MIN_ROM_SIZE)];
        rom[CARTRIDGE_TYPE_ADDRESS] = cartridge_type;
        rom[0x0149] = 0x02; // 8KB RAM by default
        rom
    }

    #[test]
    fn test_mbc2_banking_and_ram() {
        let rom = create_test_rom(0x06, MIN_ROM_SIZE * 2);
        let mut cart = Cartridge::from_rom(rom).unwrap();

        // Write to enable RAM (bit 8 must be 0)
        cart.write_rom(0x0000, 0x0A);
        assert!(cart.ram_enabled);

        // Write to select ROM bank (bit 8 must be 1)
        cart.write_rom(0x0100, 0x05);
        assert_eq!(cart.current_rom_bank(), 5);

        // RAM is only 512 bytes, masking lower 4 bits
        cart.write_ram(0xA000, 0xFF);
        assert_eq!(cart.read_ram(0xA000), 0xFF); // Because read_ram ORs with 0xF0

        cart.write_ram(0xA1FF, 0xAB);
        assert_eq!(cart.read_ram(0xA1FF), 0xFB); // 0xB | 0xF0

        // Reading past 512 bytes wraps around
        assert_eq!(cart.read_ram(0xA200), 0xFF);
    }

    #[test]
    fn test_rom_ram_direct_access() {
        let rom = create_test_rom(0x08, MIN_ROM_SIZE);
        let mut cart = Cartridge::from_rom(rom).unwrap();

        assert_eq!(cart.current_rom_bank(), 1);
        
        // No enable needed, direct access
        cart.write_ram(0xA123, 0x42);
        assert_eq!(cart.read_ram(0xA123), 0x42);
    }

    #[test]
    fn test_mbc5_rumble() {
        let rom = create_test_rom(0x1D, MIN_ROM_SIZE * 2);
        let mut cart = Cartridge::from_rom(rom).unwrap();

        cart.write_rom(0x4000, 0x0B); // 0b1011 (Rumble = 1, RAM bank = 3)
        assert!(cart.rumble_active);
        assert_eq!(cart.current_ram_bank(), 3);

        cart.write_rom(0x4000, 0x02); // 0b0010 (Rumble = 0, RAM bank = 2)
        assert!(!cart.rumble_active);
        assert_eq!(cart.current_ram_bank(), 2);
    }

    #[test]
    fn test_mbc3_rtc_latching() {
        let rom = create_test_rom(0x0F, MIN_ROM_SIZE * 2);
        let mut cart = Cartridge::from_rom(rom).unwrap();

        cart.write_rom(0x0000, 0x0A); // Enable RTC
        
        cart.write_rom(0x4000, 0x08); // Select RTC Seconds register
        cart.write_ram(0xA000, 45); // Set seconds to 45
        
        // Latch
        cart.write_rom(0x6000, 0x00);
        cart.write_rom(0x6000, 0x01);

        assert_eq!(cart.read_ram(0xA000), 45);
    }
}
