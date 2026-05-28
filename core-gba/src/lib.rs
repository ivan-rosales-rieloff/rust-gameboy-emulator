pub mod bus;
pub mod cpu;
pub mod ppu;
pub mod timer;
pub mod dma;

use std::convert::Infallible;

use bus::Bus;
use core_common::{HeadlessCore, StepResult};
use cpu::Cpu;
use ppu::{SCREEN_WIDTH, SCREEN_HEIGHT};

#[derive(Debug, Default)]
pub struct GameBoyAdvance {
    pub cpu: Cpu,
    pub bus: Bus,
    cycles: u64,
}

impl GameBoyAdvance {
    /// Creates a new Game Boy Advance emulator instance with default post-boot state.
    pub fn new() -> Self {
        let mut gba = Self::default();
        gba.cpu.init_post_boot();
        gba
    }

    /// Creates a new emulator instance with loaded ROM bytes.
    pub fn from_rom_bytes(rom: Vec<u8>) -> Result<Self, Infallible> {
        let mut gba = Self {
            cpu: Cpu::new(),
            bus: Bus::new(rom),
            cycles: 0,
        };
        gba.cpu.init_post_boot();
        Ok(gba)
    }

    /// Returns the total cycles executed since start.
    pub fn total_cycles(&self) -> u64 {
        self.cycles
    }

    /// Returns the static screen dimensions for the Game Boy Advance.
    pub fn screen_dimensions() -> (usize, usize) {
        (SCREEN_WIDTH, SCREEN_HEIGHT)
    }

    /// Returns the title of the loaded game pak, or "GBA GAME" if not set.
    pub fn title(&self) -> &str {
        // GBA ROM Header title is at offset 0xA0 to 0xAB (12 bytes)
        if self.bus.rom.len() > 0xA0 {
            let end = std::cmp::min(self.bus.rom.len(), 0xAC);
            if let Ok(title_str) = std::str::from_utf8(&self.bus.rom[0xA0..end]) {
                return title_str.trim();
            }
        }
        "GBA GAME"
    }

    /// Returns a reference to the active PPU framebuffer.
    pub fn framebuffer(&self) -> &[u32] {
        &self.bus.ppu.framebuffer
    }

    /// Updates the keypad button states.
    pub fn set_button_state(&mut self, buttons: u8) {
        self.bus.set_button_state(buttons);
    }

    /// Runs the emulator for exactly one frame's worth of cycles (280,896 cycles).
    pub fn run_frame(&mut self) -> Result<(), Infallible> {
        let mut frame_cycles = 0;
        // GBA frame has exactly 280,896 CPU cycles (1232 cycles/line * 228 lines)
        while frame_cycles < 280_896 {
            let step = self.step()?;
            frame_cycles += step.cycles;
        }
        Ok(())
    }

    /// Dummy audio helper matching Game Boy core interface.
    pub fn take_audio_samples(&mut self) -> Vec<f32> {
        Vec::new()
    }

    /// Dummy save helper matching Game Boy core interface.
    pub fn has_battery(&self) -> bool {
        false
    }

    pub fn save_game(&self) -> Result<(), Infallible> {
        Ok(())
    }
}

impl HeadlessCore for GameBoyAdvance {
    type Error = Infallible;

    fn step(&mut self) -> Result<StepResult, Self::Error> {
        println!("[DEBUG STEP] Calling cpu.step...");
        let cpu_cycles = self.cpu.step(&mut self.bus);
        println!("[DEBUG STEP] cpu.step finished, cycles = {}. Calling bus.tick...", cpu_cycles);
        self.bus.tick(cpu_cycles);
        println!("[DEBUG STEP] bus.tick finished. Advancing cycles...");
        self.cycles += cpu_cycles as u64;
        Ok(StepResult::new(cpu_cycles, self.cpu.halted))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core_common::HeadlessCore;

    #[test]
    fn step_advances_stub_cycle_counter() {
        println!("[DEBUG TEST] Creating GameBoyAdvance...");
        let mut gba = GameBoyAdvance::new();
        println!("[DEBUG TEST] GameBoyAdvance created. Calling step...");
        let step_result = gba.step().unwrap();
        println!("[DEBUG TEST] Step finished. result={:?}", step_result);

        assert!(step_result.cycles > 0);
        assert_eq!(gba.total_cycles(), step_result.cycles as u64);
    }
}
