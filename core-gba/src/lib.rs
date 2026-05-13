use std::convert::Infallible;

use core_common::{HeadlessCore, StepResult};

#[derive(Debug, Default)]
pub struct GameBoyAdvance {
    cycles: u64,
}

impl GameBoyAdvance {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn total_cycles(&self) -> u64 {
        self.cycles
    }
}

impl HeadlessCore for GameBoyAdvance {
    type Error = Infallible;

    fn step(&mut self) -> Result<StepResult, Self::Error> {
        self.cycles += 1;
        Ok(StepResult::new(1, false))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core_common::HeadlessCore;

    #[test]
    fn step_advances_stub_cycle_counter() {
        let mut gba = GameBoyAdvance::new();
        let step_result = gba.step().unwrap();

        assert_eq!(step_result.cycles, 1);
        assert_eq!(gba.total_cycles(), 1);
    }
}
