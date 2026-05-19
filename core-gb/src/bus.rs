//!
//! # Game Boy Memory Bus
//!
//! The Bus module implements the Game Boy's memory management system. The Game Boy uses
//! a 16-bit address space (64KB) that is mapped to different memory regions and I/O devices.
//!
//! ## Memory Map Architecture
//!
//! The Game Boy's address space is divided into distinct regions:
//!
//! ### ROM Area (0x0000-0x7FFF)
//! - `0x0000-0x3FFF`: ROM Bank 0 (fixed, contains game code and data)
//! - `0x4000-0x7FFF`: ROM Bank N (switchable via Memory Bank Controller)
//!
//! ### Video RAM (0x8000-0x9FFF)
//! - `0x8000-0x97FF`: Tile data (patterns for sprites and backgrounds)
//! - `0x9800-0x9BFF`: Background map (tile indices for BG display)
//! - `0x9C00-0x9FFF`: Window map (tile indices for window overlay)
//!
//! ### External RAM (0xA000-0xBFFF)
//! - Cartridge RAM for save data (battery-backed on some cartridges)
//!
//! ### Working RAM (0xC000-0xDFFF)
//! - General-purpose RAM for game variables and stack
//! - `0xC000-0xDDFF`: Main WRAM
//! - `0xDE00-0xDFFF`: Shadow WRAM (used by stack)
//!
//! ### Echo RAM (0xE000-0xFDFF)
//! - Mirror of WRAM (0xC000-0xDDFF) - writing here affects both areas
//!
//! ### OAM (Object Attribute Memory) (0xFE00-0xFE9F)
//! - Sprite attributes (position, tile index, flags) for up to 40 sprites
//!
//! ### Unusable Memory (0xFEA0-0xFEFF)
//! - Returns 0xFF on reads, writes are ignored
//!
//! ### I/O Registers (0xFF00-0xFF7F)
//! - Hardware control registers (joypad, LCD, sound, timers, etc.)
//!
//! ### High RAM (0xFF80-0xFFFE)
//! - Fast RAM for critical variables and interrupt handlers
//!
//! ### Interrupt Enable (0xFFFF)
//! - Controls which interrupts are enabled
//!
//! ## Memory Bank Controllers (MBCs)
//!
//! Most Game Boy games use MBCs to access more than 32KB of ROM and/or RAM:
//! - **MBC1**: Basic banking, up to 2MB ROM, 32KB RAM
//! - **MBC2**: Built-in RAM, up to 2MB ROM
//! - **MBC3**: RTC (Real Time Clock), up to 2MB ROM, 32KB RAM
//! - **MBC5**: Advanced banking, up to 8MB ROM, 128KB RAM
//!
//! ## I/O Register Details
//!
//! Key I/O registers implemented:
//! - `0xFF00`: P1/JOYP - Joypad input
//! - `0xFF01`: SB - Serial transfer data
//! - `0xFF02`: SC - Serial transfer control
//! - `0xFF04`: DIV - Divider register
//! - `0xFF05`: TIMA - Timer counter
//! - `0xFF06`: TMA - Timer modulo
//! - `0xFF07`: TAC - Timer control
//! - `0xFF0F`: IF - Interrupt flag
//! - `0xFF10-0xFF3F`: Sound registers (not implemented)
//! - `0xFF40`: LCDC - LCD control
//! - `0xFF41`: STAT - LCD status
//! - `0xFF42`: SCY - Scroll Y
//! - `0xFF43`: SCX - Scroll X
//! - `0xFF44`: LY - LCD Y coordinate
//! - `0xFF45`: LYC - LY compare
//! - `0xFF46`: DMA - OAM DMA transfer
//! - `0xFF47`: BGP - Background palette
//! - `0xFF48`: OBP0 - Object palette 0
//! - `0xFF49`: OBP1 - Object palette 1
//! - `0xFF4A`: WY - Window Y position
//! - `0xFF4B`: WX - Window X position
//!
//! ## Rust Implementation Notes
//!
//! - Uses fixed-size arrays for performance and memory safety
//! - Address translation uses simple offset calculations
//! - I/O registers are handled specially for hardware simulation
//! - Tracing support for debugging memory access patterns

use crate::cartridge::Cartridge;
use crate::trace::{trace, trace_enabled};

// Memory region sizes (in bytes)
const VRAM_SIZE: usize = 0x2000;  // 8KB Video RAM
const WRAM_SIZE: usize = 0x2000;  // 8KB Working RAM
const OAM_SIZE: usize = 0x00A0;   // 160 bytes Object Attribute Memory
const IO_SIZE: usize = 0x0080;    // 128 bytes I/O Registers
const HRAM_SIZE: usize = 0x007F;  // 127 bytes High RAM

/// The Game Boy memory bus that handles all memory access and I/O operations.
///
/// The Bus acts as the central hub connecting the CPU to all memory regions
/// and I/O devices. It implements the Game Boy's memory mapping and handles
/// special I/O operations like joypad input and LCD control.
///
/// ## Component Responsibilities
///
/// - **Memory Mapping**: Translates 16-bit addresses to physical memory locations
/// - **I/O Handling**: Manages hardware registers and device communication
/// - **Cartridge Interface**: Routes ROM/RAM access through MBC logic
/// - **Debugging Support**: Provides tracing for memory access patterns
#[derive(Debug, Clone)]
pub struct Bus {
    /// The game cartridge with ROM and optional RAM
    cartridge: Cartridge,
    /// Video RAM for tile data and background/window maps
    vram: [u8; VRAM_SIZE],
    /// Working RAM for game variables and stack
    wram: [u8; WRAM_SIZE],
    /// Object Attribute Memory for sprite properties
    oam: [u8; OAM_SIZE],
    /// I/O registers for hardware control
    io: [u8; IO_SIZE],
    /// High RAM for fast variable access
    hram: [u8; HRAM_SIZE],
    /// Interrupt Enable register (separate for easier access)
    ie: u8,
    /// Current joypad button state (bitfield)
    button_state: u8,
    /// Audio Processing Unit
    pub apu: crate::apu::Apu,
    /// DIV timer ticks cycle accumulator
    div_counter: u32,
    /// TIMA timer ticks cycle accumulator
    timer_counter: u32,
}

impl Bus {
    /// Creates a new memory bus with the specified cartridge.
    ///
    /// Initializes all memory regions and sets up default I/O register values
    /// that match the Game Boy's power-on state (after boot ROM execution).
    ///
    /// # Arguments
    /// * `cartridge` - The loaded game cartridge
    ///
    /// # I/O Register Initialization
    ///
    /// Sets up LCD controller with typical defaults:
    /// - LCD enabled with background and sprites on
    /// - Tile data at 0x8000, BG map at 0x9800
    /// - Default palettes and scroll positions
    pub fn new(cartridge: Cartridge) -> Self {
        // Initialize I/O registers with Game Boy post-boot defaults
        let mut io = [0; IO_SIZE];

        // Joypad input register: all buttons released
        io[0x00] = 0xFF; // P1/JOYP

        // Serial transfer registers
        io[0x01] = 0x00; // SB
        io[0x02] = 0x7E; // SC

        // Timer registers
        io[0x04] = 0x00; // DIV
        io[0x05] = 0x00; // TIMA
        io[0x06] = 0x00; // TMA
        io[0x07] = 0x00; // TAC

        // Interrupt flags
        io[0x0F] = 0xE1; // IF

        // Sound registers (boot-final defaults)
        io[0x10] = 0x80; // NR10
        io[0x11] = 0xBF; // NR11
        io[0x12] = 0xF3; // NR12
        io[0x14] = 0xBF; // NR14
        io[0x16] = 0x3F; // NR21
        io[0x17] = 0x00; // NR22
        io[0x19] = 0xBF; // NR24
        io[0x1A] = 0x7F; // NR30
        io[0x1B] = 0xFF; // NR31
        io[0x1C] = 0x9F; // NR32
        io[0x1E] = 0xBF; // NR33
        io[0x20] = 0xFF; // NR41
        io[0x21] = 0x00; // NR42
        io[0x22] = 0x00; // NR43
        io[0x23] = 0xBF; // NR44
        io[0x24] = 0x77; // NR50
        io[0x25] = 0xF3; // NR51
        io[0x26] = 0xF1; // NR52

        // LCD Control Register (LCDC) - enables LCD with standard settings
        io[0x40] = 0x91; // LCD enabled, BG enabled, OBJ enabled, tile data 8000, BG map 9800

        // LCD position and scrolling registers
        io[0x42] = 0x00; // SCY (Scroll Y) - background vertical scroll
        io[0x43] = 0x00; // SCX (Scroll X) - background horizontal scroll
        io[0x44] = 0x00; // LY (LCD Y) - current scanline (read-only, updated by PPU)
        io[0x45] = 0x00; // LYC (LY Compare)

        // Palette registers - default grayscale palettes
        io[0x47] = 0xFC; // BGP (Background Palette) - darkest to lightest gray
        io[0x48] = 0xFF; // OBP0 (Object Palette 0) - all white (transparent)
        io[0x49] = 0xFF; // OBP1 (Object Palette 1) - all white (transparent)

        // Window position registers
        io[0x4A] = 0x00; // WY (Window Y position)
        io[0x4B] = 0x00; // WX (Window X position)

        // Boot ROM disable register
        io[0x50] = 0x01; // FF50: boot ROM disabled

        Self {
            cartridge,
            vram: [0; VRAM_SIZE],
            wram: [0; WRAM_SIZE],
            oam: [0; OAM_SIZE],
            io,
            hram: [0; HRAM_SIZE],
            ie: 0,              // No interrupts enabled initially
            button_state: 0xFF, // All buttons released (active low)
            apu: crate::apu::Apu::default(),
            div_counter: 0,
            timer_counter: 0,
        }
    }

    /// Returns a reference to the cartridge for external access.
    pub fn cartridge(&self) -> &Cartridge {
        &self.cartridge
    }

    /// Updates the joypad button state.
    ///
    /// The button state is stored as a bitfield where each bit represents
    /// one button. The joypad reading logic converts this to the Game Boy's
    /// active-low format when P1 register is read.
    ///
    /// # Arguments
    /// * `buttons` - 8-bit value where bits represent button states (1 = pressed)
    ///   - Bits 0-3: Action buttons (A, B, Select, Start)
    ///   - Bits 4-7: Direction buttons (Right, Left, Up, Down)
    pub fn set_button_state(&mut self, buttons: u8) {
        self.button_state = buttons;
    }

    /// Reads a byte from the specified memory address.
    ///
    /// This is the main memory access function that implements the Game Boy's
    /// memory map. Different address ranges are routed to different memory
    /// regions or handled specially for I/O operations.
    ///
    /// # Arguments
    /// * `address` - 16-bit memory address to read from
    ///
    /// # Returns
    /// * The byte value at the specified address
    ///
    /// # Special Cases
    ///
    /// - Joypad input (0xFF00) requires special handling for button selection
    /// - Unusable memory (0xFEA0-0xFEFF) always returns 0xFF
    /// - Echo RAM mirrors WRAM
    pub fn read8(&self, address: u16) -> u8 {
        match address {
            // ROM banks (routed through cartridge MBC)
            0x0000..=0x7FFF => self.cartridge.read_rom(address),

            // Video RAM (tile data and background maps)
            0x8000..=0x9FFF => self.vram[usize::from(address - 0x8000)],

            // External RAM (cartridge save data)
            0xA000..=0xBFFF => self.cartridge.read_ram(address),

            // Working RAM with special tracing for debugging
            0xC000..=0xDFFF => {
                let value = self.wram[usize::from(address - 0xC000)];
                // Special trace for the memory location Pokemon Red polls
                if trace_enabled() && address == 0xCFC7 {
                    trace(&format!("WRAM read: 0xCFC7 = 0x{:02X}", value));
                }
                value
            }

            // Echo RAM (mirror of WRAM C000-DDFF)
            0xE000..=0xFDFF => self.wram[usize::from(address - 0xE000)],

            // Object Attribute Memory (sprite properties)
            0xFE00..=0xFE9F => self.oam[usize::from(address - 0xFE00)],

            // Unusable memory (always returns 0xFF)
            0xFEA0..=0xFEFF => 0xFF,

            // Joypad input register (special handling required)
            0xFF00 => self.read_joypad(),

            // Sound registers and Wave RAM mapped directly to APU
            0xFF10..=0xFF26 | 0xFF30..=0xFF3F => self.apu.read_register(address),

            // I/O registers with selective tracing
            0xFF01..=0xFF7F => {
                let value = self.io[usize::from(address - 0xFF00)];
                // Trace important interrupt and LCD registers
                if trace_enabled() && (address == 0xFF41 || address == 0xFF0F || address == 0xFFFF) {
                    trace(&format!("IO read: 0x{:04X} = 0x{:02X}", address, value));
                }
                value
            }

            // High RAM (fast access for critical code)
            0xFF80..=0xFFFE => self.hram[usize::from(address - 0xFF80)],

            // Interrupt Enable register
            0xFFFF => self.ie,
        }
    }

    /// Writes a byte to the specified memory address.
    ///
    /// Handles memory-mapped writes to different regions. Some addresses
    /// trigger special hardware operations (like DMA transfers).
    ///
    /// # Arguments
    /// * `address` - 16-bit memory address to write to
    /// * `value` - Byte value to write
    ///
    /// # Special Operations
    ///
    /// - OAM DMA (0xFF46): Triggers sprite data transfer from RAM to OAM
    /// - ROM writes: Handled by cartridge MBC for bank switching
    /// - Unusable memory: Writes are ignored
    pub fn write8(&mut self, address: u16, value: u8) {
        match address {
            // ROM area (handled by cartridge MBC for bank switching)
            0x0000..=0x7FFF => self.cartridge.write_rom(address, value),

            // Video RAM with background map tracing
            0x8000..=0x9FFF => {
                // Trace writes to background map area for debugging
                if trace_enabled() && address >= 0x9800 && address <= 0x9BFF {
                    trace(&format!("VRAM BG map write: 0x{:04X} <= 0x{:02X}", address, value));
                }
                self.vram[usize::from(address - 0x8000)] = value;
            }

            // External RAM (cartridge save data)
            0xA000..=0xBFFF => self.cartridge.write_ram(address, value),

            // Working RAM with special tracing
            0xC000..=0xDFFF => {
                if trace_enabled() && address == 0xCFC7 {
                    trace(&format!("WRAM write: 0xCFC7 <= 0x{:02X}", value));
                }
                self.wram[usize::from(address - 0xC000)] = value;
            }

            // Echo RAM (mirror of WRAM)
            0xE000..=0xFDFF => self.wram[usize::from(address - 0xE000)] = value,

            // Object Attribute Memory
            0xFE00..=0xFE9F => self.oam[usize::from(address - 0xFE00)] = value,

            // Unusable memory (writes ignored)
            0xFEA0..=0xFEFF => {},

            // Joypad register (P1) with tracing
            0xFF00 => {
                if trace_enabled() {
                    trace(&format!("P1 write: 0xFF00 <= 0x{value:02X}"));
                }
                self.io[0] = value;
            }

            // Sound registers and Wave RAM mapped directly to APU
            0xFF10..=0xFF26 | 0xFF30..=0xFF3F => self.apu.write_register(address, value),

            // DIV register write (writing any value resets DIV and internal divider counter)
            0xFF04 => {
                self.io[0x04] = 0;
                self.div_counter = 0;
            }

            // STAT register write protection: PPU-owned bits 0-2 are read-only to CPU writes
            0xFF41 => {
                self.io[0x41] = (self.io[0x41] & 0x07) | (value & 0x78);
            }

            // Other I/O registers
            0xFF01..=0xFF7F => {
                let io_index = usize::from(address - 0xFF00);

                // Special handling for OAM DMA transfer
                if address == 0xFF46 {
                    // DMA transfers 160 bytes from RAM page to OAM
                    // Source address = value * 0x100
                    let source = u16::from(value) << 8;
                    for offset in 0..OAM_SIZE {
                        self.oam[offset] = self.read8(source + offset as u16);
                    }
                }

                // Trace background palette changes
                if trace_enabled() && address == 0xFF47 {
                    trace(&format!("BGP write: 0xFF47 <= 0x{value:02X}"));
                }

                self.io[io_index] = value;
            }

            // High RAM
            0xFF80..=0xFFFE => self.hram[usize::from(address - 0xFF80)] = value,

            // Interrupt Enable register
            0xFFFF => self.ie = value,
        }
    }

    /// Reads the joypad input register (P1/JOYP at 0xFF00).
    ///
    /// The Game Boy joypad uses a matrix system where buttons are arranged
    /// in two groups of 4, selected by bits in the P1 register.
    ///
    /// ## Joypad Matrix
    ///
    /// ```text
    /// P1.4 = 0: Select action buttons    P1.5 = 0: Select direction buttons
    ///           P13 --- A  (Bit 0)                 P13 --- Right (Bit 0)
    ///           P12 --- B  (Bit 1)                 P12 --- Left  (Bit 1)
    ///           P11 --- Select (Bit 2)             P11 --- Up    (Bit 2)
    ///           P10 --- Start (Bit 3)              P10 --- Down  (Bit 3)
    /// ```
    ///
    /// When a select bit is 0, the corresponding button group is connected
    /// to the lower 4 bits of P1, active low (0 = pressed).
    ///
    /// # Returns
    /// * 8-bit value representing joypad state in Game Boy format
    fn read_joypad(&self) -> u8 {
        // Extract select bits from P1 register
        let select = self.io[0];
        let p14 = select & 0x20 == 0; // Action buttons selected (bit 5 = 0)
        let p15 = select & 0x10 == 0; // Direction buttons selected (bit 4 = 0)

        // Start with all buttons released (active-low 1s)
        let mut lower = 0x0F;

        if p14 {
            // Action buttons: A, B, Select, Start (lower 4 bits of button_state)
            let pressed = self.button_state & 0x0F;
            lower &= !pressed; // Invert to set active-low (0 = pressed)
        }

        if p15 {
            // Direction buttons: Right, Left, Up, Down (upper 4 bits of button_state)
            let pressed = (self.button_state >> 4) & 0x0F;
            lower &= !pressed; // Invert to set active-low (0 = pressed)
        }

        // Upper nibble contains high 2 bits set to 1, and bits 4-5 from the select write
        (select & 0x30) | 0xC0 | (lower & 0x0F)
    }

    // ─── PPU Helper Methods ───────────────────────────────────────────

    /// Reads the LCDC register (0xFF40) for the PPU.
    pub fn lcdc(&self) -> u8 {
        self.io[0x40]
    }

    /// Reads the STAT register (0xFF41) for the PPU.
    pub fn stat(&self) -> u8 {
        self.io[0x41]
    }

    /// Updates the PPU-controlled bits of STAT (mode in bits 0-1, LYC flag in bit 2).
    /// Preserves game-writable bits 3-6. This bypasses the write-protection in
    /// write8 which exists to prevent games from overwriting these PPU-owned bits.
    pub fn set_stat_ppu_bits(&mut self, mode: u8, lyc_match: bool) {
        let game_bits = self.io[0x41] & 0xF8; // Preserve bits 3-7
        let lyc_bit = if lyc_match { 0x04 } else { 0x00 };
        self.io[0x41] = game_bits | (mode & 0x03) | lyc_bit;
    }

    /// Sets the LY register (0xFF44) — current scanline.
    pub fn set_ly(&mut self, scanline: u8) {
        self.io[0x44] = scanline;
    }

    /// Reads the LYC register (0xFF45) — LY compare value.
    pub fn lyc(&self) -> u8 {
        self.io[0x45]
    }

    /// Requests an interrupt by setting flags in IF (0xFF0F).
    pub fn request_interrupt(&mut self, flag: u8) {
        self.io[0x0F] |= flag;
    }

    // ─── Timer System ─────────────────────────────────────────────────

    /// Advances the hardware timer by the given number of CPU cycles.
    ///
    /// DIV (0xFF04) increments every 256 CPU cycles.
    /// TIMA (0xFF05) increments according to frequency selected by TAC (0xFF07).
    /// When TIMA overflows, it reloads from TMA (0xFF06) and requests a Timer interrupt.
    pub fn tick_timer(&mut self, cycles: u32) {
        // DIV: increments every 256 CPU cycles
        self.div_counter += cycles;
        while self.div_counter >= 256 {
            self.div_counter -= 256;
            self.io[0x04] = self.io[0x04].wrapping_add(1);
        }

        // TIMA: only runs when TAC bit 2 (timer enable) is set
        let tac = self.io[0x07];
        if tac & 0x04 != 0 {
            let freq = match tac & 0x03 {
                0 => 1024,
                1 => 16,
                2 => 64,
                3 => 256,
                _ => unreachable!(),
            };

            self.timer_counter += cycles;
            while self.timer_counter >= freq {
                self.timer_counter -= freq;
                let (new_tima, overflow) = self.io[0x05].overflowing_add(1);
                if overflow {
                    // On overflow: reload TIMA from TMA and request interrupt
                    self.io[0x05] = self.io[0x06];
                    self.io[0x0F] |= 0x04; // Set Timer interrupt flag (IF bit 2)
                } else {
                    self.io[0x05] = new_tima;
                }
            }
        }
    }
}
