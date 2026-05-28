//!
//! # GBA Hardware Timers
//!
//! This module implements the 4 independent 16-bit timers with prescaling,
//! Cascade ticking mode, and interrupt triggers.
//!

#[derive(Debug, Clone, Default)]
pub struct Timer {
    /// Current counter value
    pub counter: u16,
    /// Reload value loaded on start/overflow
    pub reload: u16,
    /// Control register value
    pub control: u16,
    /// Divider count for prescaler ticking
    pub prescaler_cycles: u32,
    /// Was this timer running in the previous cycle
    pub active: bool,
}

#[derive(Debug, Clone, Default)]
pub struct Timers {
    pub timers: [Timer; 4],
}

impl Timers {
    pub fn new() -> Self {
        Self::default()
    }

    /// Advance timers by the given CPU cycles.
    /// Updates timer state, handles Cascade mode, and triggers interrupts.
    pub fn tick(&mut self, cycles: u32, io: &mut [u8; 0x400]) {
        let mut overflow_cascade = [false; 4];

        for i in 0..4 {
            let timer = &mut self.timers[i];

            // Parse Control Register
            let started = (timer.control & (1 << 7)) != 0;
            let cascade = i > 0 && (timer.control & (1 << 2)) != 0;

            if !started {
                timer.active = false;
                continue;
            }

            // If it just started, load counter with reload value
            if !timer.active {
                timer.counter = timer.reload;
                timer.prescaler_cycles = 0;
                timer.active = true;
            }

            let mut ticks = 0;

            if cascade {
                // In Cascade mode, ticks are driven by the overflow of the previous timer
                if overflow_cascade[i - 1] {
                    ticks = 1;
                }
            } else {
                // Standard mode: ticks are driven by CPU cycles and prescaler
                let prescaler_limit = match timer.control & 3 {
                    0 => 1,
                    1 => 64,
                    2 => 256,
                    3 => 1024,
                    _ => 1,
                };

                timer.prescaler_cycles += cycles;
                ticks = timer.prescaler_cycles / prescaler_limit;
                timer.prescaler_cycles %= prescaler_limit;
            }

            if ticks > 0 {
                let old_counter = timer.counter;
                let (new_counter, overflow) = old_counter.overflowing_add(ticks as u16);

                if overflow || new_counter < old_counter {
                    timer.counter = timer.reload.wrapping_add((ticks - 1) as u16);
                    overflow_cascade[i] = true;

                    // Trigger Timer Interrupt (Bit 6 in TMxCNT)
                    if (timer.control & (1 << 6)) != 0 {
                        let interrupt_bit = 3 + i as u8; // Bit 3,4,5,6 in IF for Timer 0,1,2,3
                        let current_if = io[0x202] as u16 | ((io[0x203] as u16) << 8);
                        let new_if = current_if | (1 << interrupt_bit);
                        io[0x202] = new_if as u8;
                        io[0x203] = (new_if >> 8) as u8;
                    }
                } else {
                    timer.counter = new_counter;
                }
            }
        }
    }
}
