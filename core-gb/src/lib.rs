mod bus;
mod cartridge;
mod cpu;
mod ppu;

use std::error::Error;
use std::fmt::{Display, Formatter};

use bus::Bus;
use core_common::{run_steps, HeadlessCore, RunStats, StepResult};
use cpu::{Cpu, CpuError};
use ppu::{Ppu, SCREEN_HEIGHT, SCREEN_WIDTH};
pub use cartridge::{Cartridge, CartridgeError};

pub use cpu::Registers;

#[derive(Debug)]
pub struct GameBoy {
    cpu: Cpu,
    bus: Bus,
    ppu: Ppu,
    cycles: u64,
}

impl GameBoy {
    pub fn from_rom_bytes(rom: Vec<u8>) -> Result<Self, GameBoyError> {
        let cartridge = Cartridge::from_rom(rom)?;
        Ok(Self {
            cpu: Cpu::default(),
            bus: Bus::new(cartridge),
            ppu: Ppu::default(),
            cycles: 0,
        })
    }

    pub fn title(&self) -> &str {
        self.bus.cartridge().title()
    }

    pub fn pc(&self) -> u16 {
        self.cpu.pc()
    }

    pub fn registers(&self) -> Registers {
        self.cpu.registers()
    }

    pub fn total_cycles(&self) -> u64 {
        self.cycles
    }

    pub fn screen_dimensions() -> (usize, usize) {
        (SCREEN_WIDTH, SCREEN_HEIGHT)
    }

    pub fn framebuffer(&self) -> &[u8] {
        self.ppu.framebuffer()
    }

    pub fn run_steps(&mut self, steps: usize) -> Result<RunStats, GameBoyError> {
        run_steps(self, steps)
    }

    pub fn run_frame(&mut self) -> Result<(), GameBoyError> {
        loop {
            let step_result = self.step()?;
            if self.ppu.step(step_result.cycles, &self.bus) {
                return Ok(());
            }
        }
    }

    pub fn set_button_state(&mut self, buttons: u8) {
        self.bus.set_button_state(buttons);
    }
}

impl HeadlessCore for GameBoy {
    type Error = GameBoyError;

    fn step(&mut self) -> Result<StepResult, Self::Error> {
        let step_result = self.cpu.step(&mut self.bus)?;
        self.cycles += u64::from(step_result.cycles);
        Ok(step_result)
    }
}

#[derive(Debug)]
pub enum GameBoyError {
    Cartridge(CartridgeError),
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

impl From<CartridgeError> for GameBoyError {
    fn from(value: CartridgeError) -> Self {
        Self::Cartridge(value)
    }
}

impl From<CpuError> for GameBoyError {
    fn from(value: CpuError) -> Self {
        Self::Cpu(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const ROM_SIZE: usize = 0x8000;

    fn make_rom(program: &[u8], cartridge_type: u8, title: &str) -> Vec<u8> {
        let mut rom = vec![0; ROM_SIZE];
        rom[0x0147] = cartridge_type;

        let title_bytes = title.as_bytes();
        let title_len = title_bytes.len().min(0x10);
        rom[0x0134..0x0134 + title_len].copy_from_slice(&title_bytes[..title_len]);

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
        let rom = make_rom(&[0x00], 0x12, "BADTYPE");
        let error = GameBoy::from_rom_bytes(rom).unwrap_err();

        match error {
            GameBoyError::Cartridge(CartridgeError::UnsupportedCartridgeType(value)) => {
                assert_eq!(value, 0x12);
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
}
