#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StepResult {
    pub cycles: u32,
    pub halted: bool,
}

impl StepResult {
    pub const fn new(cycles: u32, halted: bool) -> Self {
        Self { cycles, halted }
    }
}

pub trait HeadlessCore {
    type Error;

    fn step(&mut self) -> Result<StepResult, Self::Error>;
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct RunStats {
    pub instructions: u64,
    pub cycles: u64,
    pub halted: bool,
}

pub fn run_steps<C: HeadlessCore>(core: &mut C, steps: usize) -> Result<RunStats, C::Error> {
    let mut stats = RunStats::default();

    for _ in 0..steps {
        let step = core.step()?;
        stats.instructions += 1;
        stats.cycles += u64::from(step.cycles);
        stats.halted = step.halted;

        if step.halted {
            break;
        }
    }

    Ok(stats)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FakeCore {
        script: Vec<StepResult>,
        cursor: usize,
    }

    impl HeadlessCore for FakeCore {
        type Error = ();

        fn step(&mut self) -> Result<StepResult, Self::Error> {
            let value = self
                .script
                .get(self.cursor)
                .copied()
                .unwrap_or(StepResult::new(4, false));
            self.cursor += 1;
            Ok(value)
        }
    }

    #[test]
    fn run_steps_tracks_instruction_and_cycle_totals() {
        let mut core = FakeCore {
            script: vec![
                StepResult::new(4, false),
                StepResult::new(8, false),
                StepResult::new(4, false),
            ],
            cursor: 0,
        };

        let stats = run_steps(&mut core, 3).unwrap();

        assert_eq!(stats.instructions, 3);
        assert_eq!(stats.cycles, 16);
        assert!(!stats.halted);
    }

    #[test]
    fn run_steps_stops_when_core_halts() {
        let mut core = FakeCore {
            script: vec![
                StepResult::new(4, false),
                StepResult::new(4, true),
                StepResult::new(4, false),
            ],
            cursor: 0,
        };

        let stats = run_steps(&mut core, 10).unwrap();

        assert_eq!(stats.instructions, 2);
        assert_eq!(stats.cycles, 8);
        assert!(stats.halted);
    }
}
