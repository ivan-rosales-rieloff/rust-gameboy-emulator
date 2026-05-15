//!
//! # Game Boy Emulator Core
//!
//! This module implements a cycle-accurate Game Boy emulator in Rust. The Game Boy is a handheld
//! gaming console released by Nintendo in 1989, featuring an 8-bit Sharp LR35902 CPU (similar to
//! Intel 8080/Zilog Z80), 8KB of working RAM, and a custom graphics processor.
//!
//! ## Game Boy Architecture Overview
//!
//! The Game Boy consists of several key components:
//! - **CPU (Sharp LR35902)**: 8-bit processor running at ~4.19 MHz, 8 registers (A, B, C, D, E, F, H, L)
//! - **Memory Management Unit (MMU)**: Handles memory mapping and I/O registers
//! - **Picture Processing Unit (PPU)**: Generates video output, handles sprites and backgrounds
//! - **Audio Processing Unit (APU)**: Generates sound (not implemented in this emulator)
//! - **Cartridge Interface**: Supports various memory bank controllers (MBC1, MBC3, etc.)
//! - **Timer and Interrupt System**: Handles timing and asynchronous events
//!
//! ## Memory Map
//!
//! The Game Boy uses a 16-bit address space (64KB total):
//! - `0x0000-0x3FFF`: ROM Bank 0 (fixed)
//! - `0x4000-0x7FFF`: ROM Bank N (switchable)
//! - `0x8000-0x9FFF`: Video RAM (VRAM) for tiles and maps
//! - `0xA000-0xBFFF`: Cartridge RAM (external save data)
//! - `0xC000-0xDFFF`: Working RAM (WRAM)
//! - `0xE000-0xFDFF`: Echo RAM (mirror of WRAM)
//! - `0xFE00-0xFE9F`: Object Attribute Memory (OAM) for sprites
//! - `0xFEA0-0xFEFF`: Unusable memory
//! - `0xFF00-0xFF7F`: I/O Registers
//! - `0xFF80-0xFFFE`: High RAM (HRAM)
//! - `0xFFFF`: Interrupt Enable Register
//!
//! ## Rust Implementation Design
//!
//! This emulator uses a component-based architecture where each hardware component
//! (CPU, PPU, Bus, Cartridge) is implemented as a separate module with clear interfaces.
//! The `GameBoy` struct coordinates all components and provides the main emulation loop.
//!
//! Key design decisions:
//! - **Cycle Accuracy**: All operations are timed in CPU cycles for accurate timing
//! - **Borrow Checking**: Rust's ownership system prevents memory corruption
//! - **Error Handling**: Comprehensive error types for different failure modes
//! - **Modularity**: Each component can be tested and modified independently

mod bus;
mod cartridge;
mod cpu;
mod ppu;
mod trace;

use std::error::Error;
use std::fmt::{Display, Formatter};

use bus::Bus;
use core_common::{run_steps, HeadlessCore, RunStats, StepResult};
use cpu::{Cpu, CpuError};
use ppu::{Ppu, SCREEN_HEIGHT, SCREEN_WIDTH};
pub use cartridge::{Cartridge, CartridgeError};

pub use cpu::Registers;

/// The main Game Boy emulator struct.
///
/// This struct represents a complete Game Boy system with all its components.
/// It implements the `HeadlessCore` trait for integration with the common emulator framework.
///
/// ## Component Coordination
///
/// The Game Boy runs by alternating between CPU and PPU steps:
/// 1. CPU executes instructions (variable cycles)
/// 2. PPU processes graphics for the same number of cycles
/// 3. Repeat until a frame is complete (when PPU signals frame done)
///
/// This ensures that CPU and PPU stay synchronized, which is crucial for
/// proper graphics timing and interrupt handling.
#[derive(Debug)]
pub struct GameBoy {
    /// The central processing unit that executes Game Boy instructions
    cpu: Cpu,
    /// The memory bus that handles all memory access and I/O operations
    bus: Bus,
    /// The picture processing unit that generates video output
    ppu: Ppu,
    /// Total CPU cycles executed since emulator start (for performance tracking)
    cycles: u64,
}

impl GameBoy {
    /// Creates a new Game Boy emulator from ROM data.
    ///
    /// This function initializes all components with their default states and
    /// loads the provided ROM into a cartridge. The Game Boy starts in a
    /// standard boot state (similar to the real hardware after boot ROM execution).
    ///
    /// # Arguments
    /// * `rom` - The complete ROM data as a byte vector
    ///
    /// # Returns
    /// * `Ok(GameBoy)` - Successfully initialized emulator
    /// * `Err(GameBoyError)` - Failed to load cartridge (invalid ROM format, etc.)
    ///
    /// # Example
    /// ```rust
    /// let rom_data = std::fs::read("PokemonRed.gb")?;
    /// let gb = GameBoy::from_rom_bytes(rom_data)?;
    /// ```
    pub fn from_rom_bytes(rom: Vec<u8>) -> Result<Self, GameBoyError> {
        // Parse the ROM into a cartridge (handles MBC detection, header validation)
        let cartridge = Cartridge::from_rom(rom)?;

        Ok(Self {
            // CPU starts with default register values (PC=0x0100, SP=0xFFFE, etc.)
            cpu: Cpu::default(),
            // Bus connects all components and handles memory mapping
            bus: Bus::new(cartridge),
            // PPU starts in default state (LCD off, blank screen)
            ppu: Ppu::default(),
            // Cycle counter for performance statistics
            cycles: 0,
        })
    }

    /// Returns the game title from the cartridge header.
    ///
    /// The Game Boy cartridge header (at offset 0x0134-0x0143) contains
    /// an ASCII title string that identifies the game.
    pub fn title(&self) -> &str {
        self.bus.cartridge().title()
    }

    /// Returns the current program counter value.
    ///
    /// The PC (Program Counter) points to the next instruction to execute.
    /// This is useful for debugging and understanding program flow.
    pub fn pc(&self) -> u16 {
        self.cpu.pc()
    }

    /// Returns a copy of all CPU registers.
    ///
    /// The Game Boy has 8 registers: A, F, B, C, D, E, H, L.
    /// F is the flags register (Zero, Negative, Half-Carry, Carry).
    pub fn registers(&self) -> Registers {
        self.cpu.registers()
    }

    /// Returns the total number of CPU cycles executed.
    ///
    /// This is used for performance monitoring and can help identify
    /// if the emulator is running at the correct speed.
    pub fn total_cycles(&self) -> u64 {
        self.cycles
    }

    /// Returns the screen dimensions in pixels.
    ///
    /// Game Boy screen is 160x144 pixels (20x18 tiles of 8x8 pixels each).
    /// This is a static method since screen size never changes.
    pub fn screen_dimensions() -> (usize, usize) {
        (SCREEN_WIDTH, SCREEN_HEIGHT)
    }

    /// Returns a reference to the current framebuffer.
    ///
    /// The framebuffer contains the raw pixel data for the current frame.
    /// Each pixel is represented as a single byte (palette index 0-3).
    /// The array is SCREEN_WIDTH * SCREEN_HEIGHT bytes.
    pub fn framebuffer(&self) -> &[u8] {
        self.ppu.framebuffer()
    }

    /// Runs the emulator for a specified number of CPU steps.
    ///
    /// This is used by automated testing and debugging tools.
    /// Each step executes one CPU instruction and updates all components.
    pub fn run_steps(&mut self, steps: usize) -> Result<RunStats, GameBoyError> {
        run_steps(self, steps)
    }

    /// Runs the emulator until one complete frame is rendered.
    ///
    /// This is the main emulation loop for real-time gameplay.
    /// A frame takes approximately 70224 CPU cycles (59.73 FPS).
    ///
    /// The function alternates between CPU and PPU steps until the PPU
    /// signals that a frame is complete (scanline 144 reached).
    pub fn run_frame(&mut self) -> Result<(), GameBoyError> {
        loop {
            // Execute one CPU instruction
            let step_result = self.step()?;

            // Advance PPU by the same number of cycles
            // Returns true when a frame is complete
            if self.ppu.step(step_result.cycles, &mut self.bus) {
                return Ok(());
            }
        }
    }

    /// Updates the joypad button state.
    ///
    /// The Game Boy has 8 buttons: Right, Left, Up, Down, A, B, Select, Start.
    /// Each bit in the `buttons` byte represents one button state.
    ///
    /// # Arguments
    /// * `buttons` - Bitfield where each bit represents a button (1 = pressed)
    pub fn set_button_state(&mut self, buttons: u8) {
        self.bus.set_button_state(buttons);
    }

    /// Saves the current game state to persistent storage.
    ///
    /// Only works for cartridges with battery-backed RAM.
    /// The save data is written to a .sav file alongside the ROM.
    pub fn save_game(&self) -> Result<(), GameBoyError> {
        self.bus.cartridge().save_game()?;
        Ok(())
    }

    /// Returns true if the cartridge has battery-backed save RAM.
    ///
    /// Battery cartridges can save game progress between sessions.
    /// This affects whether save_game() will do anything useful.
    pub fn has_battery(&self) -> bool {
        self.bus.cartridge().has_battery()
    }

    /// Returns true if interrupts are globally enabled (IME flag).
    ///
    /// The Game Boy has a master interrupt enable flag that can disable
    /// all interrupts. This is used for critical sections of code.
    pub fn ime_enabled(&self) -> bool {
        self.cpu.ime_enabled()
    }
}

/// Implementation of the HeadlessCore trait for integration with the emulator framework.
///
/// This trait provides the basic stepping interface that allows the Game Boy
/// to be used with common emulator utilities like automated testing and debugging.
impl HeadlessCore for GameBoy {
    type Error = GameBoyError;

    /// Executes a single CPU instruction and returns timing information.
    ///
    /// This is the core of the emulation loop. Each step:
    /// 1. Executes one CPU instruction
    /// 2. Updates the cycle counter
    /// 3. Returns timing info for PPU synchronization
    ///
    /// # Returns
    /// * `StepResult` containing cycles executed and any special events
    fn step(&mut self) -> Result<StepResult, Self::Error> {
        // Execute one CPU instruction (may take 4-24 cycles)
        let step_result = self.cpu.step(&mut self.bus)?;

        // Track total cycles for performance monitoring
        self.cycles += u64::from(step_result.cycles);

        Ok(step_result)
    }
}

/// Comprehensive error type for all Game Boy emulator failures.
///
/// This enum wraps errors from different subsystems, providing a unified
/// error interface while preserving specific error details.
#[derive(Debug)]
pub enum GameBoyError {
    /// Errors related to cartridge loading and MBC handling
    Cartridge(CartridgeError),
    /// Errors from CPU execution (invalid opcodes, etc.)
    Cpu(CpuError),
}

impl Display for GameBoyError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Cartridge(error) => write!(f, "cartridge error: {error}"),
            Self::Cpu(error) => write!(f, "cpu error: {error}"),
        }
    }
}

impl Error for GameBoyError {}

/// Automatic conversion from CartridgeError to GameBoyError.
///
/// This allows using `?` operator in functions that return GameBoyError
/// when calling cartridge-related functions.
impl From<CartridgeError> for GameBoyError {
    fn from(value: CartridgeError) -> Self {
        Self::Cartridge(value)
    }
}

/// Automatic conversion from CpuError to GameBoyError.
///
/// This allows using `?` operator in functions that return GameBoyError
/// when calling CPU-related functions.
impl From<CpuError> for GameBoyError {
    fn from(value: CpuError) -> Self {
        Self::Cpu(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Size of a minimal Game Boy ROM (32KB)
    const ROM_SIZE: usize = 0x8000;

    /// Creates a minimal test ROM with the specified program and metadata.
    ///
    /// This is used for unit testing CPU instructions and emulator behavior.
    /// The function creates a valid cartridge header and places the test
    /// program at the entry point (0x0100).
    ///
    /// # Arguments
    /// * `program` - Machine code bytes to place at entry point
    /// * `cartridge_type` - MBC type (0x00 = no MBC)
    /// * `title` - Game title (max 16 characters)
    ///
    /// # Returns
    /// * Complete ROM data as byte vector
    fn make_rom(program: &[u8], cartridge_type: u8, title: &str) -> Vec<u8> {
        // Start with empty ROM
        let mut rom = vec![0; ROM_SIZE];

        // Set cartridge type in header (determines MBC)
        rom[0x0147] = cartridge_type;

        // Set game title in header (ASCII, max 16 chars)
        let title_bytes = title.as_bytes();
        let title_len = title_bytes.len().min(0x10);
        rom[0x0134..0x0134 + title_len].copy_from_slice(&title_bytes[..title_len]);

        // Place program at entry point (after boot ROM would normally jump here)
        let entry_point = 0x0100usize;
        let program_len = program.len().min(ROM_SIZE - entry_point);
        rom[entry_point..entry_point + program_len].copy_from_slice(&program[..program_len]);

        rom
    }

    #[test]
    fn loads_rom_only_cartridge_and_title() {
        let rom = make_rom(&[0x00], 0x00, "POKEMON");
        let game_boy = GameBoy::from_rom_bytes(rom).unwrap();

        assert_eq!(game_boy.title(), "POKEMON");
        assert_eq!(game_boy.pc(), 0x0100);
    }

    #[test]
    fn rejects_unsupported_cartridge_type() {
        let rom = make_rom(&[0x00], 0x10, "BADTYPE");
        let error = GameBoy::from_rom_bytes(rom).unwrap_err();

        match error {
            GameBoyError::Cartridge(CartridgeError::UnsupportedCartridgeType(value)) => {
                assert_eq!(value, 0x10);
            }
            _ => panic!("expected unsupported cartridge type error"),
        }
    }

    #[test]
    fn steps_nop_program_deterministically() {
        let rom = make_rom(&[0x00, 0x00, 0x00, 0x00], 0x00, "NOPS");
        let mut game_boy = GameBoy::from_rom_bytes(rom).unwrap();
        let stats = game_boy.run_steps(4).unwrap();

        assert_eq!(stats.instructions, 4);
        assert_eq!(stats.cycles, 16);
        assert_eq!(game_boy.pc(), 0x0104);
    }

    #[test]
    fn supports_immediate_load_and_memory_roundtrip() {
        let rom = make_rom(
            &[
                0x3E, 0x42, // LD A,$42
                0xEA, 0x00, 0xC0, // LD ($C000),A
                0x3E, 0x00, // LD A,$00
                0xFA, 0x00, 0xC0, // LD A,($C000)
                0x76, // HALT
            ],
            0x00,
            "ROUNDTRIP",
        );
        let mut game_boy = GameBoy::from_rom_bytes(rom).unwrap();
        let stats = game_boy.run_steps(10).unwrap();

        assert_eq!(stats.instructions, 5);
        assert_eq!(stats.cycles, 52);
        assert!(stats.halted);
        assert_eq!(game_boy.registers().a, 0x42);
    }

    #[test]
    fn supports_mbc1_rom_bank_switching() {
        let mut rom = make_rom(&[0x00], 0x01, "BANKING");
        let switch_bank_program = [
            0x3E, 0x01, // LD A,$01
            0xEA, 0x00, 0x20, // LD ($2000),A -> MBC1 ROM bank set to 1
            0xFA, 0x00, 0x40, // LD A,($4000)
            0x76, // HALT
        ];
        let bank1_data = 0x99;
        rom[0x0100..0x0100 + switch_bank_program.len()]
            .copy_from_slice(&switch_bank_program);
        rom[0x4000] = bank1_data;

        let mut game_boy = GameBoy::from_rom_bytes(rom).unwrap();
        let stats = game_boy.run_steps(5).unwrap();

        assert_eq!(stats.instructions, 4);
        assert!(stats.halted);
        assert_eq!(game_boy.registers().a, bank1_data);
    }

    #[test]
    fn supports_battery_backed_cartridge_detection() {
        // Test MBC1+RAM+Battery cartridge type 0x03
        let rom = make_rom(&[0x00], 0x03, "BATTERYTEST");
        let game_boy = GameBoy::from_rom_bytes(rom).unwrap();

        assert!(game_boy.has_battery());
        assert_eq!(game_boy.title(), "BATTERYTEST");
    }

    #[test]
    fn supports_mbc3_ram_battery_cartridge_detection() {
        // Test MBC3+RAM+Battery cartridge type 0x13
        let rom = make_rom(&[0x00], 0x13, "POKEMONRED");
        let game_boy = GameBoy::from_rom_bytes(rom).unwrap();

        assert!(game_boy.has_battery());
        assert_eq!(game_boy.title(), "POKEMONRED");
    }

    #[test]
    fn debug_pokemon_red_lcdc_initialization() {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../PokemonRed.gb");
        let rom = std::fs::read(&path).unwrap();
        let mut game_boy = GameBoy::from_rom_bytes(rom).unwrap();

        for frame in 0..5 {
            game_boy.run_frame().unwrap();
            let lcdc = game_boy.bus.read8(0xFF40);
            let scroll_y = game_boy.bus.read8(0xFF42);
            let scroll_x = game_boy.bus.read8(0xFF43);
            let palette = game_boy.bus.read8(0xFF47);
            let framebuffer = game_boy.framebuffer();
            let min = *framebuffer.iter().min().unwrap();
            let max = *framebuffer.iter().max().unwrap();
            println!("frame={} pc=0x{:04X} lcdc=0x{lcdc:02X} scy=0x{scroll_y:02X} scx=0x{scroll_x:02X} pal=0x{palette:02X} min={} max={}", frame, game_boy.pc(), min, max);
        }
    }

    #[test]
    fn trace_pokemon_red_initial_instructions() {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../PokemonRed.gb");
        let rom = std::fs::read(&path).unwrap();
        let mut game_boy = GameBoy::from_rom_bytes(rom).unwrap();

        for _ in 0..50 {
            let pc = game_boy.pc();
            let opcode = game_boy.bus.read8(pc);
            println!("pc=0x{:04X} opcode=0x{:02X}", pc, opcode);
            match game_boy.cpu.step(&mut game_boy.bus) {
                Ok(step) => {
                    println!("  cycles={} halted={}", step.cycles, step.halted);
                    if step.halted {
                        break;
                    }
                }
                Err(err) => {
                    panic!("CPU error: {err}");
                }
            }
        }

        // Continue running frames to reach title screen
        println!("Continuing to title screen...");
        for frame in 0..100 {
            game_boy.run_frame().unwrap();
            if frame % 10 == 0 {
                let pc = game_boy.pc();
                let lcdc = game_boy.bus.read8(0xFF40);
                let palette = game_boy.bus.read8(0xFF47);
                let framebuffer = game_boy.framebuffer();
                let min = *framebuffer.iter().min().unwrap();
                let max = *framebuffer.iter().max().unwrap();
                println!("frame={} pc=0x{:04X} lcdc=0x{lcdc:02X} pal=0x{palette:02X} min={} max={}", frame, pc, min, max);
            }
        }

        // Simulate pressing Start button
        println!("Pressing Start button...");
        game_boy.set_button_state(0x08); // BTN_START

        // Run a few more frames with Start held
        for frame in 100..150 {
            game_boy.run_frame().unwrap();
            let pc = game_boy.pc();
            let lcdc = game_boy.bus.read8(0xFF40);
            let palette = game_boy.bus.read8(0xFF47);
            let framebuffer = game_boy.framebuffer();
            let min = *framebuffer.iter().min().unwrap();
            let max = *framebuffer.iter().max().unwrap();
            println!("frame={} pc=0x{:04X} lcdc=0x{lcdc:02X} pal=0x{palette:02X} min={} max={}", frame, pc, min, max);
        }
    }

    #[test]
    fn supports_non_battery_cartridge_detection() {
        // Test MBC1 cartridge type 0x01 (no battery)
        let rom = make_rom(&[0x00], 0x01, "NOBATTERY");
        let game_boy = GameBoy::from_rom_bytes(rom).unwrap();

        assert!(!game_boy.has_battery());
        assert_eq!(game_boy.title(), "NOBATTERY");
    }

    #[test]
    fn supports_add_instruction_and_direct_loads() {
        let rom = make_rom(
            &[
                0x3E, 0x05, // LD A,$05
                0x06, 0x03, // LD B,$03
                0x80, // ADD A,B
                0x76, // HALT
            ],
            0x00,
            "ARITH",
        );
        let mut game_boy = GameBoy::from_rom_bytes(rom).unwrap();
        let stats = game_boy.run_steps(4).unwrap();

        assert_eq!(stats.instructions, 4);
        assert!(stats.halted);
        assert_eq!(game_boy.registers().a, 0x08);
    }

    #[test]
    fn supports_cp_immediate_instruction() {
        let rom = make_rom(
            &[
                0x3E, 0x05, // LD A,$05
                0xFE, 0x05, // CP $05
                0x76, // HALT
            ],
            0x00,
            "COMPARE",
        );
        let mut game_boy = GameBoy::from_rom_bytes(rom).unwrap();
        let stats = game_boy.run_steps(3).unwrap();

        assert_eq!(stats.instructions, 3);
        assert!(stats.halted);
        assert_eq!(game_boy.registers().f & 0x80, 0x80); // Z flag set
        assert_eq!(game_boy.registers().f & 0x40, 0x40); // N flag set
        assert_eq!(game_boy.registers().a, 0x05);
    }
}
