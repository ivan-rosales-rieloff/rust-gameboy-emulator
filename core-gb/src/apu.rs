//! # Game Boy Audio Processing Unit (APU)
//!
//! Implements a cycle-accurate Game Boy sound synthesizer (44,100 Hz stereo).
//! The Game Boy APU consists of 4 independent audio channels:
//! - **Channel 1 (Pulse 1)**: Square wave with volume envelope, length counter, and frequency sweep.
//! - **Channel 2 (Pulse 2)**: Square wave with volume envelope and length counter.
//! - **Channel 3 (Wave)**: Custom wave pattern RAM with volume shift and length counter.
//! - **Channel 4 (Noise)**: White noise via Linear Feedback Shift Register (LFSR) with volume envelope.

const SAMPLE_RATE: f64 = 44100.0;
const CPU_CLOCK_HZ: f64 = 4194304.0;
const CYCLES_PER_SAMPLE: f64 = CPU_CLOCK_HZ / SAMPLE_RATE;

// Duty cycles patterns (each duty represents 8 steps of 0 or 1)
const DUTY_PATTERNS: [[u8; 8]; 4] = [
    [0, 0, 0, 0, 0, 0, 0, 1], // 12.5%
    [1, 0, 0, 0, 0, 0, 0, 1], // 25.0%
    [1, 0, 0, 0, 0, 1, 1, 1], // 50.0%
    [0, 1, 1, 1, 1, 1, 1, 0], // 75.0%
];

#[derive(Debug, Clone, Default)]
struct PulseChannel {
    enabled: bool,
    dac_enabled: bool,
    frequency: u16,
    timer: i32,
    duty: u8,
    duty_step: usize,

    // Length counter
    length_counter: u16,
    length_enabled: bool,

    // Envelope
    envelope_initial_volume: u8,
    envelope_volume: u8,
    envelope_direction: bool, // true = increase, false = decrease
    envelope_period: u8,
    envelope_timer: u8,

    // Sweep (Channel 1 only)
    has_sweep: bool,
    sweep_period: u8,
    sweep_timer: u8,
    sweep_direction: bool, // true = subtract, false = add
    sweep_shift: u8,
    shadow_frequency: u16,
    sweep_enabled: bool,
}

impl PulseChannel {
    fn new(has_sweep: bool) -> Self {
        Self {
            has_sweep,
            ..Default::default()
        }
    }

    fn tick_frequency(&mut self, cycles: i32) {
        if !self.enabled {
            return;
        }
        self.timer -= cycles;
        while self.timer <= 0 {
            let period = (2048 - self.frequency as i32) * 4;
            self.timer += period.max(4); // Prevent infinite loop if period is too small
            self.duty_step = (self.duty_step + 1) % 8;
        }
    }

    fn tick_length(&mut self) {
        if self.length_enabled && self.length_counter > 0 {
            self.length_counter -= 1;
            if self.length_counter == 0 {
                self.enabled = false;
            }
        }
    }

    fn tick_envelope(&mut self) {
        if self.envelope_period == 0 {
            return;
        }

        if self.envelope_timer > 0 {
            self.envelope_timer -= 1;
            if self.envelope_timer == 0 {
                self.envelope_timer = self.envelope_period;
                if self.envelope_direction {
                    if self.envelope_volume < 15 {
                        self.envelope_volume += 1;
                    }
                } else {
                    if self.envelope_volume > 0 {
                        self.envelope_volume -= 1;
                    }
                }
            }
        }
    }

    fn tick_sweep(&mut self) {
        if !self.has_sweep || !self.sweep_enabled || self.sweep_period == 0 {
            return;
        }

        if self.sweep_timer > 0 {
            self.sweep_timer -= 1;
            if self.sweep_timer == 0 {
                self.sweep_timer = self.sweep_period;
                if let Some(new_freq) = self.calculate_sweep_frequency() {
                    if new_freq <= 2047 && self.sweep_shift > 0 {
                        self.frequency = new_freq;
                        self.shadow_frequency = new_freq;
                        // Perform an immediate second overflow check
                        if self.calculate_sweep_frequency().is_none() {
                            self.enabled = false;
                        }
                    }
                } else {
                    self.enabled = false;
                }
            }
        }
    }

    fn calculate_sweep_frequency(&self) -> Option<u16> {
        let delta = self.shadow_frequency >> self.sweep_shift;
        let new_freq = if self.sweep_direction {
            self.shadow_frequency.checked_sub(delta)
        } else {
            self.shadow_frequency.checked_add(delta)
        };

        match new_freq {
            Some(freq) if freq <= 2047 => Some(freq),
            _ => None, // Frequency overflow, disables the channel
        }
    }

    fn trigger(&mut self) {
        self.enabled = true;
        if self.length_counter == 0 {
            self.length_counter = 64;
        }

        self.timer = (2048 - self.frequency as i32) * 4;
        self.envelope_volume = self.envelope_initial_volume;
        self.envelope_timer = self.envelope_period;

        if self.has_sweep {
            self.shadow_frequency = self.frequency;
            self.sweep_timer = if self.sweep_period > 0 { self.sweep_period } else { 8 };
            self.sweep_enabled = self.sweep_period > 0 || self.sweep_shift > 0;
            if self.sweep_shift > 0 && self.calculate_sweep_frequency().is_none() {
                self.enabled = false;
            }
        }
    }

    fn sample(&self) -> f32 {
        if !self.enabled || !self.dac_enabled {
            return 0.0;
        }

        let duty_val = DUTY_PATTERNS[self.duty as usize][self.duty_step];
        if duty_val != 0 {
            (self.envelope_volume as f32) / 15.0
        } else {
            0.0
        }
    }
}

#[derive(Debug, Clone, Default)]
struct WaveChannel {
    enabled: bool,
    dac_enabled: bool,
    frequency: u16,
    timer: i32,
    wave_index: usize,
    wave_ram: [u8; 16],

    // Length counter
    length_counter: u16,
    length_enabled: bool,

    // Volume level shift (0 = mute, 1 = 100%, 2 = 50%, 3 = 25%)
    volume_shift: u8,
}

impl WaveChannel {
    fn tick_frequency(&mut self, cycles: i32) {
        if !self.enabled {
            return;
        }
        self.timer -= cycles;
        while self.timer <= 0 {
            let period = (2048 - self.frequency as i32) * 2;
            self.timer += period.max(2);
            self.wave_index = (self.wave_index + 1) % 32;
        }
    }

    fn tick_length(&mut self) {
        if self.length_enabled && self.length_counter > 0 {
            self.length_counter -= 1;
            if self.length_counter == 0 {
                self.enabled = false;
            }
        }
    }

    fn trigger(&mut self) {
        self.enabled = true;
        if self.length_counter == 0 {
            self.length_counter = 256;
        }
        self.timer = (2048 - self.frequency as i32) * 2;
        self.wave_index = 0;
    }

    fn sample(&self) -> f32 {
        if !self.enabled || !self.dac_enabled || self.volume_shift == 0 {
            return 0.0;
        }

        // 32 4-bit samples are packed in 16 bytes of wave_ram
        let byte_index = self.wave_index / 2;
        let packed_byte = self.wave_ram[byte_index];
        let sample_4bit = if self.wave_index % 2 == 0 {
            packed_byte >> 4
        } else {
            packed_byte & 0x0F
        };

        // Shift volume according to NR32 volume code
        // 0 = Mute, 1 = 100% (shift 0), 2 = 50% (shift 1), 3 = 25% (shift 2)
        let shifted = sample_4bit >> (self.volume_shift - 1);
        (shifted as f32) / 15.0
    }
}

#[derive(Debug, Clone)]
struct NoiseChannel {
    enabled: bool,
    dac_enabled: bool,
    timer: i32,
    lfsr: u16,
    lfsr_7bit: bool,

    // Length counter
    length_counter: u16,
    length_enabled: bool,

    // Envelope
    envelope_initial_volume: u8,
    envelope_volume: u8,
    envelope_direction: bool,
    envelope_period: u8,
    envelope_timer: u8,

    // Noise ratio parameters
    dividing_ratio: u8,
    shift_clock_frequency: u8,
}

impl Default for NoiseChannel {
    fn default() -> Self {
        Self {
            enabled: false,
            dac_enabled: false,
            timer: 0,
            lfsr: 0x7FFF, // Standard initial state for LFSR
            lfsr_7bit: false,
            length_counter: 0,
            length_enabled: false,
            envelope_initial_volume: 0,
            envelope_volume: 0,
            envelope_direction: false,
            envelope_period: 0,
            envelope_timer: 0,
            dividing_ratio: 0,
            shift_clock_frequency: 0,
        }
    }
}

impl NoiseChannel {
    fn tick_frequency(&mut self, cycles: i32) {
        if !self.enabled {
            return;
        }
        self.timer -= cycles;
        while self.timer <= 0 {
            let div = if self.dividing_ratio == 0 {
                8
            } else {
                self.dividing_ratio as i32 * 16
            };
            let period = div << self.shift_clock_frequency;
            self.timer += period.max(8);

            // Shift LFSR
            let bit0 = self.lfsr & 1;
            let bit1 = (self.lfsr >> 1) & 1;
            let xor_result = bit0 ^ bit1;
            self.lfsr = (self.lfsr >> 1) | (xor_result << 14);

            if self.lfsr_7bit {
                self.lfsr = (self.lfsr & !(1 << 6)) | (xor_result << 6);
            }
        }
    }

    fn tick_length(&mut self) {
        if self.length_enabled && self.length_counter > 0 {
            self.length_counter -= 1;
            if self.length_counter == 0 {
                self.enabled = false;
            }
        }
    }

    fn tick_envelope(&mut self) {
        if self.envelope_period == 0 {
            return;
        }

        if self.envelope_timer > 0 {
            self.envelope_timer -= 1;
            if self.envelope_timer == 0 {
                self.envelope_timer = self.envelope_period;
                if self.envelope_direction {
                    if self.envelope_volume < 15 {
                        self.envelope_volume += 1;
                    }
                } else {
                    if self.envelope_volume > 0 {
                        self.envelope_volume -= 1;
                    }
                }
            }
        }
    }

    fn trigger(&mut self) {
        self.enabled = true;
        if self.length_counter == 0 {
            self.length_counter = 64;
        }
        let div = if self.dividing_ratio == 0 {
            8
        } else {
            self.dividing_ratio as i32 * 16
        };
        self.timer = div << self.shift_clock_frequency;
        self.lfsr = 0x7FFF;
        self.envelope_volume = self.envelope_initial_volume;
        self.envelope_timer = self.envelope_period;
    }

    fn sample(&self) -> f32 {
        if !self.enabled || !self.dac_enabled {
            return 0.0;
        }

        // Noise sample output is determined by bit 0 of the LFSR (active low)
        if (self.lfsr & 1) == 0 {
            (self.envelope_volume as f32) / 15.0
        } else {
            0.0
        }
    }
}

/// The main Audio Processing Unit (APU) containing the synthesis and control state.
#[derive(Debug, Clone)]
pub struct Apu {
    channel1: PulseChannel,
    channel2: PulseChannel,
    channel3: WaveChannel,
    channel4: NoiseChannel,

    // Master settings
    nr50: u8, // Master volume & Vin selection
    nr51: u8, // Panning selections
    nr52: u8, // Sound on/off master switch

    // Clock trackers
    frame_sequencer_cycle: u32,
    frame_sequencer_step: u8,
    sample_accumulator: f64,

    // Audio output buffer (alternating Left and Right samples)
    samples: Vec<f32>,
}

impl Default for Apu {
    fn default() -> Self {
        Self {
            channel1: PulseChannel::new(true),
            channel2: PulseChannel::new(false),
            channel3: WaveChannel::default(),
            channel4: NoiseChannel::default(),
            nr50: 0x77,
            nr51: 0xF3,
            nr52: 0xF1, // Bit 7 set (APU power enabled), Ch1 flag set initially
            frame_sequencer_cycle: 0,
            frame_sequencer_step: 0,
            sample_accumulator: 0.0,
            samples: Vec::with_capacity(4096),
        }
    }
}

impl Apu {
    /// Resets all APU states. Used when powering down the APU (NR52 Bit 7 = 0).
    fn clear_state(&mut self) {
        self.channel1 = PulseChannel::new(true);
        self.channel2 = PulseChannel::new(false);
        self.channel3 = WaveChannel::default();
        self.channel4 = NoiseChannel::default();
        self.nr50 = 0;
        self.nr51 = 0;
        self.nr52 = 0;
        self.frame_sequencer_cycle = 0;
        self.frame_sequencer_step = 0;
    }

    /// Ticks the APU state by the given number of CPU cycles.
    /// Synthesizes stereo audio samples dynamically into the internal buffer.
    pub fn tick(&mut self, cycles: u32) {
        // If master APU switch is disabled, don't generate any sound
        if (self.nr52 & 0x80) == 0 {
            // Keep the cycle accumulator ticking to avoid massive sample backlogs
            self.sample_accumulator += cycles as f64;
            while self.sample_accumulator >= CYCLES_PER_SAMPLE {
                self.sample_accumulator -= CYCLES_PER_SAMPLE;
                self.samples.push(0.0);
                self.samples.push(0.0);
            }
            return;
        }

        let cycles_i32 = cycles as i32;

        // Tick internal channels frequency timers
        self.channel1.tick_frequency(cycles_i32);
        self.channel2.tick_frequency(cycles_i32);
        self.channel3.tick_frequency(cycles_i32);
        self.channel4.tick_frequency(cycles_i32);

        // Tick 512 Hz Frame Sequencer
        self.frame_sequencer_cycle += cycles;
        while self.frame_sequencer_cycle >= 8192 {
            self.frame_sequencer_cycle -= 8192;
            self.tick_frame_sequencer();
        }

        // Downsampling: accumulate cycles and generate samples at 44,100 Hz
        self.sample_accumulator += cycles as f64;
        while self.sample_accumulator >= CYCLES_PER_SAMPLE {
            self.sample_accumulator -= CYCLES_PER_SAMPLE;
            let (left_sample, right_sample) = self.mix();
            self.samples.push(left_sample);
            self.samples.push(right_sample);
        }
    }

    /// Ticks the frame sequencer (512 Hz) to modulate length, sweep, and envelope.
    fn tick_frame_sequencer(&mut self) {
        match self.frame_sequencer_step {
            0 | 4 => {
                // Length
                self.channel1.tick_length();
                self.channel2.tick_length();
                self.channel3.tick_length();
                self.channel4.tick_length();
            }
            2 | 6 => {
                // Length & Sweep
                self.channel1.tick_length();
                self.channel2.tick_length();
                self.channel3.tick_length();
                self.channel4.tick_length();

                self.channel1.tick_sweep();
            }
            7 => {
                // Volume Envelope
                self.channel1.tick_envelope();
                self.channel2.tick_envelope();
                self.channel4.tick_envelope();
            }
            _ => {}
        }
        self.frame_sequencer_step = (self.frame_sequencer_step + 1) % 8;
    }

    /// Mixes and pans the four channels into a stereo (Left, Right) sample pair.
    fn mix(&self) -> (f32, f32) {
        let ch1 = self.channel1.sample();
        let ch2 = self.channel2.sample();
        let ch3 = self.channel3.sample();
        let ch4 = self.channel4.sample();

        let mut left = 0.0f32;
        let mut right = 0.0f32;

        // Channel 1 Panning
        if (self.nr51 & 0x10) != 0 { left += ch1; }
        if (self.nr51 & 0x01) != 0 { right += ch1; }

        // Channel 2 Panning
        if (self.nr51 & 0x20) != 0 { left += ch2; }
        if (self.nr51 & 0x02) != 0 { right += ch2; }

        // Channel 3 Panning
        if (self.nr51 & 0x40) != 0 { left += ch3; }
        if (self.nr51 & 0x04) != 0 { right += ch3; }

        // Channel 4 Panning
        if (self.nr51 & 0x80) != 0 { left += ch4; }
        if (self.nr51 & 0x08) != 0 { right += ch4; }

        // Average output
        left /= 4.0;
        right /= 4.0;

        // Master Volume Scaling (0..=7)
        let left_vol = ((self.nr50 >> 4) & 0x07) as f32 / 7.0;
        let right_vol = (self.nr50 & 0x07) as f32 / 7.0;

        (left * left_vol, right * right_vol)
    }

    /// Exposes and clears the accumulated audio samples buffer for the frontend playback.
    pub fn take_samples(&mut self) -> Vec<f32> {
        std::mem::take(&mut self.samples)
    }

    /// Handles Game Boy sound registers read operations.
    pub fn read_register(&self, address: u16) -> u8 {
        // If powered down, registers return 0 (except some bits of NR52)
        if (self.nr52 & 0x80) == 0 && address != 0xFF26 {
            return 0;
        }

        match address {
            // Pulse 1 Sweep
            0xFF10 => {
                let dir = if self.channel1.sweep_direction { 0x08 } else { 0x00 };
                0x80 | (self.channel1.sweep_period << 4) | dir | self.channel1.sweep_shift
            }
            // Pulse 1 Length/Duty
            0xFF11 => 0x3F | (self.channel1.duty << 6),
            // Pulse 1 Envelope
            0xFF12 => {
                let dir = if self.channel1.envelope_direction { 0x08 } else { 0x00 };
                (self.channel1.envelope_initial_volume << 4) | dir | self.channel1.envelope_period
            }
            // Pulse 1 Frequency High / Control
            0xFF14 => {
                let len_bit = if self.channel1.length_enabled { 0x40 } else { 0x00 };
                0xBF | len_bit
            }

            // Pulse 2 Length/Duty
            0xFF16 => 0x3F | (self.channel2.duty << 6),
            // Pulse 2 Envelope
            0xFF17 => {
                let dir = if self.channel2.envelope_direction { 0x08 } else { 0x00 };
                (self.channel2.envelope_initial_volume << 4) | dir | self.channel2.envelope_period
            }
            // Pulse 2 Frequency High / Control
            0xFF19 => {
                let len_bit = if self.channel2.length_enabled { 0x40 } else { 0x00 };
                0xBF | len_bit
            }

            // Wave Channel DAC
            0xFF1A => {
                let dac_bit = if self.channel3.dac_enabled { 0x80 } else { 0x00 };
                0x7F | dac_bit
            }
            // Wave Volume Level
            0xFF1C => 0x9F | (self.channel3.volume_shift << 5),
            // Wave Frequency High / Control
            0xFF1E => {
                let len_bit = if self.channel3.length_enabled { 0x40 } else { 0x00 };
                0xBF | len_bit
            }

            // Noise Volume Envelope
            0xFF21 => {
                let dir = if self.channel4.envelope_direction { 0x08 } else { 0x00 };
                (self.channel4.envelope_initial_volume << 4) | dir | self.channel4.envelope_period
            }
            // Noise Polynomial Counter (LFSR)
            0xFF22 => {
                let step = if self.channel4.lfsr_7bit { 0x08 } else { 0x00 };
                (self.channel4.shift_clock_frequency << 4) | step | self.channel4.dividing_ratio
            }
            // Noise Control
            0xFF23 => {
                let len_bit = if self.channel4.length_enabled { 0x40 } else { 0x00 };
                0xBF | len_bit
            }

            // Master selection registers
            0xFF24 => self.nr50,
            0xFF25 => self.nr51,
            // Master ON/OFF Status
            0xFF26 => {
                let pwr = self.nr52 & 0x80;
                let ch1 = if self.channel1.enabled { 0x01 } else { 0x00 };
                let ch2 = if self.channel2.enabled { 0x02 } else { 0x00 };
                let ch3 = if self.channel3.enabled { 0x04 } else { 0x00 };
                let ch4 = if self.channel4.enabled { 0x08 } else { 0x00 };
                pwr | 0x70 | ch1 | ch2 | ch3 | ch4
            }

            // Custom Wave RAM
            0xFF30..=0xFF3F => self.channel3.wave_ram[usize::from(address - 0xFF30)],

            _ => 0xFF,
        }
    }

    /// Handles Game Boy sound registers write operations.
    pub fn write_register(&mut self, address: u16, value: u8) {
        // If master switch is powered off, writes are ignored (except power toggle write to NR52)
        if (self.nr52 & 0x80) == 0 && address != 0xFF26 {
            // However, on real Game Boy length counters are still writable even when powered down
            if address == 0xFF11 {
                self.channel1.length_counter = 64 - (value & 0x3F) as u16;
            } else if address == 0xFF16 {
                self.channel2.length_counter = 64 - (value & 0x3F) as u16;
            } else if address == 0xFF1B {
                self.channel3.length_counter = 256 - value as u16;
            } else if address == 0xFF20 {
                self.channel4.length_counter = 64 - (value & 0x3F) as u16;
            }
            return;
        }

        match address {
            // Pulse 1 Sweep
            0xFF10 => {
                self.channel1.sweep_period = (value >> 4) & 0x07;
                self.channel1.sweep_direction = (value & 0x08) != 0;
                self.channel1.sweep_shift = value & 0x07;
            }
            // Pulse 1 Duty & length counter
            0xFF11 => {
                self.channel1.duty = value >> 6;
                self.channel1.length_counter = 64 - (value & 0x3F) as u16;
            }
            // Pulse 1 Envelope
            0xFF12 => {
                self.channel1.envelope_initial_volume = value >> 4;
                self.channel1.envelope_direction = (value & 0x08) != 0;
                self.channel1.envelope_period = value & 0x07;
                self.channel1.dac_enabled = (value & 0xF8) != 0;
                if !self.channel1.dac_enabled {
                    self.channel1.enabled = false;
                }
            }
            // Pulse 1 Frequency Low
            0xFF13 => {
                self.channel1.frequency = (self.channel1.frequency & 0x0700) | value as u16;
            }
            // Pulse 1 Frequency High & Trigger
            0xFF14 => {
                self.channel1.frequency = (self.channel1.frequency & 0x00FF) | ((value as u16 & 0x07) << 8);
                self.channel1.length_enabled = (value & 0x40) != 0;
                if (value & 0x80) != 0 {
                    self.channel1.trigger();
                }
            }

            // Pulse 2 Duty & length counter
            0xFF16 => {
                self.channel2.duty = value >> 6;
                self.channel2.length_counter = 64 - (value & 0x3F) as u16;
            }
            // Pulse 2 Envelope
            0xFF17 => {
                self.channel2.envelope_initial_volume = value >> 4;
                self.channel2.envelope_direction = (value & 0x08) != 0;
                self.channel2.envelope_period = value & 0x07;
                self.channel2.dac_enabled = (value & 0xF8) != 0;
                if !self.channel2.dac_enabled {
                    self.channel2.enabled = false;
                }
            }
            // Pulse 2 Frequency Low
            0xFF18 => {
                self.channel2.frequency = (self.channel2.frequency & 0x0700) | value as u16;
            }
            // Pulse 2 Frequency High & Trigger
            0xFF19 => {
                self.channel2.frequency = (self.channel2.frequency & 0x00FF) | ((value as u16 & 0x07) << 8);
                self.channel2.length_enabled = (value & 0x40) != 0;
                if (value & 0x80) != 0 {
                    self.channel2.trigger();
                }
            }

            // Wave DAC control
            0xFF1A => {
                self.channel3.dac_enabled = (value & 0x80) != 0;
                if !self.channel3.dac_enabled {
                    self.channel3.enabled = false;
                }
            }
            // Wave Length counter
            0xFF1B => {
                self.channel3.length_counter = 256 - value as u16;
            }
            // Wave Volume Level Select
            0xFF1C => {
                self.channel3.volume_shift = (value >> 5) & 0x03;
            }
            // Wave Frequency Low
            0xFF1D => {
                self.channel3.frequency = (self.channel3.frequency & 0x0700) | value as u16;
            }
            // Wave Frequency High & Trigger
            0xFF1E => {
                self.channel3.frequency = (self.channel3.frequency & 0x00FF) | ((value as u16 & 0x07) << 8);
                self.channel3.length_enabled = (value & 0x40) != 0;
                if (value & 0x80) != 0 {
                    self.channel3.trigger();
                }
            }

            // Noise Length counter
            0xFF20 => {
                self.channel4.length_counter = 64 - (value & 0x3F) as u16;
            }
            // Noise Volume Envelope
            0xFF21 => {
                self.channel4.envelope_initial_volume = value >> 4;
                self.channel4.envelope_direction = (value & 0x08) != 0;
                self.channel4.envelope_period = value & 0x07;
                self.channel4.dac_enabled = (value & 0xF8) != 0;
                if !self.channel4.dac_enabled {
                    self.channel4.enabled = false;
                }
            }
            // Noise Polynomial Counter (LFSR config)
            0xFF22 => {
                self.channel4.shift_clock_frequency = value >> 4;
                self.channel4.lfsr_7bit = (value & 0x08) != 0;
                self.channel4.dividing_ratio = value & 0x07;
            }
            // Noise Trigger
            0xFF23 => {
                self.channel4.length_enabled = (value & 0x40) != 0;
                if (value & 0x80) != 0 {
                    self.channel4.trigger();
                }
            }

            // Master Volume Select
            0xFF24 => self.nr50 = value,
            // Stereo panning mappings
            0xFF25 => self.nr51 = value,
            // Master ON/OFF Switch
            0xFF26 => {
                let old_pwr = self.nr52 & 0x80;
                let new_pwr = value & 0x80;
                self.nr52 = (self.nr52 & 0x7F) | new_pwr;

                if old_pwr != 0 && new_pwr == 0 {
                    // Powered down: clear registers and synthesis states
                    self.clear_state();
                } else if old_pwr == 0 && new_pwr != 0 {
                    // Powered up: frame sequencer is reset to step 0
                    self.frame_sequencer_step = 0;
                }
            }

            // Wave RAM Custom samples
            0xFF30..=0xFF3F => {
                self.channel3.wave_ram[usize::from(address - 0xFF30)] = value;
            }

            _ => {}
        }
    }
}
