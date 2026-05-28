//!
//! # GBA CPU Core (ARM7TDMI)
//!
//! This module implements the Game Boy Advance's central processing unit:
//! a 32-bit ARM7TDMI RISC processor running at 16.78 MHz.
//!
//! ## ARM7TDMI Architecture Overview
//!
//! The ARM7TDMI is a 32-bit RISC processor with:
//! - **32-bit Address & Data Bus**: Supports 4GB address space
//! - **Dual Instruction Sets**:
//!   - **ARM State**: 32-bit instructions aligned to 4-byte boundaries (standard execution)
//!   - **Thumb State**: 16-bit instructions aligned to 2-byte boundaries (compact execution)
//! - **37 Registers**: 31 general-purpose 32-bit registers, 6 status registers
//! - **7 Processor Modes**: Swaps active banked registers depending on the mode
//! - **3-Stage Pipeline**: Fetch, Decode, Execute
//!

/// The 7 operating modes of the ARM7TDMI processor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CpuMode {
    User = 0b10000,
    Fiq = 0b10001,
    Irq = 0b10010,
    Supervisor = 0b10011,
    Abort = 0b10111,
    Undefined = 0b11011,
    System = 0b11111,
}

impl Default for CpuMode {
    fn default() -> Self {
        CpuMode::System
    }
}

impl CpuMode {
    /// Returns the raw mode bits for the status register.
    pub fn bits(self) -> u32 {
        self as u32
    }
}

/// Helper to map a CPU mode to a banking index for R13/R14/SPSR arrays.
#[inline]
fn banked_index(mode: CpuMode) -> usize {
    match mode {
        CpuMode::User | CpuMode::System => 0,
        CpuMode::Fiq => 1,
        CpuMode::Irq => 2,
        CpuMode::Supervisor => 3,
        CpuMode::Abort => 4,
        CpuMode::Undefined => 5,
    }
}

/// Represents the Current or Saved Program Status Register (CPSR / SPSR).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct StatusRegister {
    /// Negative flag (N) - Bit 31
    pub n: bool,
    /// Zero flag (Z) - Bit 30
    pub z: bool,
    /// Carry flag (C) - Bit 29
    pub c: bool,
    /// Overflow flag (V) - Bit 28
    pub v: bool,
    /// IRQ disable (I) - Bit 7
    pub i: bool,
    /// FIQ disable (F) - Bit 6
    pub f: bool,
    /// State flag (T) - Bit 5 (true = Thumb state, false = ARM state)
    pub t: bool,
    /// Current processor mode - Bits 0-4
    pub mode: CpuMode,
}

impl StatusRegister {
    /// Creates a default Status Register initialized to privileged System mode in ARM state.
    pub fn new() -> Self {
        Self {
            n: false,
            z: false,
            c: false,
            v: false,
            i: true,  // Interrupts disabled on startup
            f: true,  // Fast interrupts disabled on startup
            t: false, // Start in ARM state
            mode: CpuMode::System,
        }
    }

    /// Packs the status flags into a standard 32-bit register value.
    pub fn to_u32(self) -> u32 {
        let mut val = 0u32;
        if self.n { val |= 1 << 31; }
        if self.z { val |= 1 << 30; }
        if self.c { val |= 1 << 29; }
        if self.v { val |= 1 << 28; }
        if self.i { val |= 1 << 7; }
        if self.f { val |= 1 << 6; }
        if self.t { val |= 1 << 5; }
        val |= self.mode.bits() & 0x1F;
        val
    }

    /// Unpacks a 32-bit status register value.
    pub fn from_u32(&mut self, val: u32) {
        self.n = (val & (1 << 31)) != 0;
        self.z = (val & (1 << 30)) != 0;
        self.c = (val & (1 << 29)) != 0;
        self.v = (val & (1 << 28)) != 0;
        self.i = (val & (1 << 7)) != 0;
        self.f = (val & (1 << 6)) != 0;
        self.t = (val & (1 << 5)) != 0;
        self.mode = match val & 0x1F {
            0b10000 => CpuMode::User,
            0b10001 => CpuMode::Fiq,
            0b10010 => CpuMode::Irq,
            0b10011 => CpuMode::Supervisor,
            0b10111 => CpuMode::Abort,
            0b11011 => CpuMode::Undefined,
            _ => CpuMode::System,
        };
    }
}

/// Represents the complete 37-register file of the ARM7TDMI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Registers {
    /// R0-R7: Unbanked general-purpose registers (shared by all modes)
    pub r0_r7: [u32; 8],
    /// R8-R12: User / System / SVC / IRQ / ABT / UND general-purpose registers
    pub r8_r12: [u32; 5],
    /// R8-R12: Banked FIQ registers
    pub r8_r12_fiq: [u32; 5],
    /// R13 (SP): Banked stack pointers for [User/System, FIQ, IRQ, SVC, ABT, UND]
    pub r13: [u32; 6],
    /// R14 (LR): Banked link registers for [User/System, FIQ, IRQ, SVC, ABT, UND]
    pub r14: [u32; 6],
    /// R15 (PC): Shared program counter
    pub r15: u32,
    /// CPSR: Current Program Status Register
    pub cpsr: StatusRegister,
    /// Banked SPSRs (Saved Program Status Registers) for [None, FIQ, IRQ, SVC, ABT, UND]
    pub spsr: [StatusRegister; 6],
    /// Tracks if the PC was written to during instruction execution
    pub pc_written: bool,
}

impl Default for Registers {
    fn default() -> Self {
        Self {
            r0_r7: [0; 8],
            r8_r12: [0; 5],
            r8_r12_fiq: [0; 5],
            r13: [0x03007F00, 0, 0, 0, 0, 0], // Initialize SP for User/System to top of IWRAM
            r14: [0; 6],
            r15: 0,
            cpsr: StatusRegister::new(),
            spsr: [StatusRegister::default(); 6],
            pc_written: false,
        }
    }
}

impl Registers {
    /// Returns the value of a register in the current execution mode.
    #[inline]
    pub fn get(&self, reg: u8) -> u32 {
        let mode = self.cpsr.mode;
        match reg {
            0..=7 => self.r0_r7[reg as usize],
            8..=12 => {
                if mode == CpuMode::Fiq {
                    self.r8_r12_fiq[(reg - 8) as usize]
                } else {
                    self.r8_r12[(reg - 8) as usize]
                }
            }
            13 => self.r13[banked_index(mode)],
            14 => self.r14[banked_index(mode)],
            15 => self.r15,
            _ => panic!("Invalid register lookup: R{}", reg),
        }
    }

    /// Sets the value of a register in the current execution mode.
    #[inline]
    pub fn set(&mut self, reg: u8, val: u32) {
        let mode = self.cpsr.mode;
        match reg {
            0..=7 => self.r0_r7[reg as usize] = val,
            8..=12 => {
                if mode == CpuMode::Fiq {
                    self.r8_r12_fiq[(reg - 8) as usize] = val;
                } else {
                    self.r8_r12[(reg - 8) as usize] = val;
                }
            }
            13 => self.r13[banked_index(mode)] = val,
            14 => self.r14[banked_index(mode)] = val,
            15 => {
                // Ensure PC alignment
                let alignment = if self.cpsr.t { 1 } else { 3 };
                self.r15 = val & !alignment;
                self.pc_written = true;
            }
            _ => panic!("Invalid register update: R{}", reg),
        }
    }

    /// Returns the SPSR for the active privileged mode.
    pub fn get_spsr(&self) -> StatusRegister {
        let idx = banked_index(self.cpsr.mode);
        if idx == 0 {
            panic!("User or System mode does not have an SPSR");
        }
        self.spsr[idx]
    }

    /// Sets the SPSR for the active privileged mode.
    pub fn set_spsr(&mut self, val: StatusRegister) {
        let idx = banked_index(self.cpsr.mode);
        if idx == 0 {
            panic!("User or System mode does not have an SPSR");
        }
        self.spsr[idx] = val;
    }

    /// Performs a mode transition and handles banked register swaps and CPSR updates.
    pub fn switch_mode(&mut self, new_mode: CpuMode) {
        self.cpsr.mode = new_mode;
    }
}

/// The core ARM7TDMI CPU execution module.
#[derive(Debug, Default)]
pub struct Cpu {
    /// Internal registers representation
    pub registers: Registers,
    /// CPU halted state waiting for interrupt (WFI equivalent)
    pub halted: bool,
}

impl Cpu {
    /// Creates a new CPU instance.
    pub fn new() -> Self {
        Self::default()
    }

    /// Simulates high-level boot initialization matching the standard GBA post-boot environment.
    pub fn init_post_boot(&mut self) {
        self.registers.switch_mode(CpuMode::System);
        self.registers.set(13, 0x03007F00); // System/User SP
        
        self.registers.switch_mode(CpuMode::Supervisor);
        self.registers.set(13, 0x03007FE0); // Supervisor SP
        
        self.registers.switch_mode(CpuMode::Irq);
        self.registers.set(13, 0x03007FA0); // IRQ SP
        
        self.registers.switch_mode(CpuMode::System); // Boot into System mode
        
        // Post-boot Entry PC is normally 0x08000000 (Start of Game Pak ROM)
        self.registers.r15 = 0x08000000;
        self.registers.cpsr.t = false; // Start in ARM state
    }

    /// Retrieves PC value + pipeline lookahead offset depending on Thumb or ARM execution state.
    #[inline]
    pub fn pc_lookahead(&self) -> u32 {
        if self.registers.cpsr.t {
            self.registers.r15.wrapping_add(4) // Thumb mode PC is advanced by 4 (2 instructions ahead)
        } else {
            self.registers.r15.wrapping_add(8) // ARM mode PC is advanced by 8 (2 instructions ahead)
        }
    }

    /// Checks for pending interrupts and executes the IRQ transition if enabled.
    pub fn check_interrupts(&mut self, bus: &mut crate::bus::Bus) {
        let ime = bus.read_halfword(0x04000208) & 1;
        let ie = bus.read_halfword(0x04000200);
        let ip = bus.read_halfword(0x04000202);

        let pending = ie & ip;
        if pending != 0 {
            if self.halted {
                self.halted = false;
            }

            if !self.registers.cpsr.i && ime != 0 {
                let old_cpsr = self.registers.cpsr;
                self.registers.switch_mode(CpuMode::Irq);
                self.registers.set_spsr(old_cpsr);

                self.registers.cpsr.t = false;
                self.registers.cpsr.i = true;

                // LR_irq is PC + 4 (ARM) or PC + 2 (Thumb)
                let lr_val = if old_cpsr.t {
                    self.registers.r15.wrapping_add(2)
                } else {
                    self.registers.r15.wrapping_add(4)
                };
                self.registers.set(14, lr_val);

                // Branch to IRQ vector
                self.registers.r15 = 0x00000018;
                self.registers.pc_written = true;
            }
        }
    }

    /// Advances the CPU execution by a single instruction.
    /// Fetches from the bus, decodes, and executes.
    /// Returns the cycles consumed.
    pub fn step(&mut self, bus: &mut crate::bus::Bus) -> u32 {
        println!("[DEBUG CPU] step entry. PC=0x{:08X}, Mode={:?}, T={}", self.registers.r15, self.registers.cpsr.mode, self.registers.cpsr.t);
        self.check_interrupts(bus);

        if self.halted {
            println!("[DEBUG CPU] CPU is halted.");
            return 1;
        }

        // Intercept BIOS IRQ handler entry
        if self.registers.r15 == 0x00000018 {
            let handler = bus.read_word(0x03007FFC);
            println!("[DEBUG CPU] Intercepted 0x18. Handler=0x{:08X}", handler);
            if handler != 0 {
                let lr_irq = self.registers.get(14);

                // Switch to System mode
                self.registers.switch_mode(CpuMode::System);

                // Push R0-R3, R12, LR_sys, and lr_irq to System Stack
                let mut sp = self.registers.get(13);
                sp = sp.wrapping_sub(4); bus.write_word(sp, lr_irq);
                sp = sp.wrapping_sub(4); bus.write_word(sp, self.registers.get(14)); // LR_sys
                sp = sp.wrapping_sub(4); bus.write_word(sp, self.registers.get(12));
                sp = sp.wrapping_sub(4); bus.write_word(sp, self.registers.get(3));
                sp = sp.wrapping_sub(4); bus.write_word(sp, self.registers.get(2));
                sp = sp.wrapping_sub(4); bus.write_word(sp, self.registers.get(1));
                sp = sp.wrapping_sub(4); bus.write_word(sp, self.registers.get(0));
                self.registers.set(13, sp);

                // Set LR_sys to 0x0000001C (dummy return address)
                self.registers.set(14, 0x0000001C);

                // Jump to handler
                self.registers.r15 = handler;
                self.registers.pc_written = true;
            }
        }

        // Intercept BIOS IRQ handler exit
        if self.registers.r15 == 0x0000001C {
            println!("[DEBUG CPU] Intercepted 0x1C. Returning from IRQ.");
            let mut sp = self.registers.get(13);
            let r0 = bus.read_word(sp); sp = sp.wrapping_add(4);
            let r1 = bus.read_word(sp); sp = sp.wrapping_add(4);
            let r2 = bus.read_word(sp); sp = sp.wrapping_add(4);
            let r3 = bus.read_word(sp); sp = sp.wrapping_add(4);
            let r12 = bus.read_word(sp); sp = sp.wrapping_add(4);
            let lr_sys = bus.read_word(sp); sp = sp.wrapping_add(4);
            let lr_irq = bus.read_word(sp); sp = sp.wrapping_add(4);

            self.registers.set(13, sp);
            self.registers.set(0, r0);
            self.registers.set(1, r1);
            self.registers.set(2, r2);
            self.registers.set(3, r3);
            self.registers.set(12, r12);
            self.registers.set(14, lr_sys);

            // Switch back to IRQ mode to cleanly exit
            self.registers.switch_mode(CpuMode::Irq);
            let spsr = self.registers.get_spsr();
            self.registers.cpsr = spsr;

            // Return from IRQ (typically SUBS PC, LR, #4 which exits to lr_irq - 4)
            self.registers.r15 = lr_irq.wrapping_sub(4);
            self.registers.pc_written = true;
        }

        let pc = self.registers.r15;
        self.registers.pc_written = false;

        let cycles = if self.registers.cpsr.t {
            let instruction = bus.read_halfword(pc);
            println!("[DEBUG CPU] Thumb instruction: 0x{:04X} at 0x{:08X}", instruction, pc);
            let c = self.execute_thumb(instruction, bus);
            println!("[DEBUG CPU] Thumb execution finished. cycles={}", c);
            c
        } else {
            let instruction = bus.read_word(pc);
            println!("[DEBUG CPU] ARM instruction: 0x{:08X} at 0x{:08X}", instruction, pc);
            let c = self.execute_arm(instruction, bus);
            println!("[DEBUG CPU] ARM execution finished. cycles={}", c);
            c
        };

        if !self.registers.pc_written {
            let advance = if self.registers.cpsr.t { 2 } else { 4 };
            self.registers.r15 = pc.wrapping_add(advance);
            println!("[DEBUG CPU] Advancing PC by {} to 0x{:08X}", advance, self.registers.r15);
        }

        cycles
    }

    /// Dispatches a high-level software interrupt (SWI) based on standard BIOS SWI numbers.
    pub fn handle_hle_swi(&mut self, swi_number: u8, bus: &mut crate::bus::Bus) {
        match swi_number {
            0x01 => {
                // RegisterRamReset
                let flags = self.registers.get(0);
                self.register_ram_reset(flags, bus);
            }
            0x02 => {
                // Halt
                self.halted = true;
            }
            0x05 => {
                // VBlankIntrWait
                // Sets halted state to await next VBlank interrupt tick
                self.halted = true;
            }
            0x0B => {
                // CpuSet
                let src = self.registers.get(0);
                let dest = self.registers.get(1);
                let count_control = self.registers.get(2);
                self.cpu_set(src, dest, count_control, bus);
            }
            0x0C => {
                // CpuFastSet
                let src = self.registers.get(0);
                let dest = self.registers.get(1);
                let count_control = self.registers.get(2);
                self.cpu_fast_set(src, dest, count_control, bus);
            }
            _ => {
                eprintln!("[HLE SWI] Unimplemented GBA SWI: 0x{:02X}", swi_number);
            }
        }
    }

    fn register_ram_reset(&mut self, flags: u32, bus: &mut crate::bus::Bus) {
        if flags & 0x01 != 0 {
            bus.ewram.fill(0);
        }
        if flags & 0x02 != 0 {
            // Keep the last 0x200 bytes of IWRAM (stores stacks/IntrFrame)
            bus.iwram[0..(0x8000 - 0x200)].fill(0);
        }
        if flags & 0x04 != 0 {
            bus.palette_ram.fill(0);
        }
        if flags & 0x08 != 0 {
            bus.vram.fill(0);
        }
        if flags & 0x10 != 0 {
            bus.oam.fill(0);
        }
        if flags & 0x20 != 0 {
            bus.io.fill(0);
        }
    }

    fn cpu_set(&mut self, mut src: u32, mut dest: u32, count_control: u32, bus: &mut crate::bus::Bus) {
        let count = count_control & 0x001FFFFF;
        let is_32bit = (count_control & (1 << 26)) != 0;
        let fixed_src = (count_control & (1 << 24)) != 0;

        if is_32bit {
            for _ in 0..count {
                let val = bus.read_word(src);
                bus.write_word(dest, val);
                dest = dest.wrapping_add(4);
                if !fixed_src {
                    src = src.wrapping_add(4);
                }
            }
        } else {
            for _ in 0..count {
                let val = bus.read_halfword(src);
                bus.write_halfword(dest, val);
                dest = dest.wrapping_add(2);
                if !fixed_src {
                    src = src.wrapping_add(2);
                }
            }
        }
    }

    fn cpu_fast_set(&mut self, mut src: u32, mut dest: u32, count_control: u32, bus: &mut crate::bus::Bus) {
        let count = count_control & 0x001FFFFF;
        let fixed_src = (count_control & (1 << 24)) != 0;

        for _ in 0..count {
            let val = bus.read_word(src);
            bus.write_word(dest, val);
            dest = dest.wrapping_add(4);
            if !fixed_src {
                src = src.wrapping_add(4);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registers_get_set() {
        let mut regs = Registers::default();
        regs.cpsr.mode = CpuMode::User;
        
        regs.set(0, 100);
        regs.set(7, 700);
        assert_eq!(regs.get(0), 100);
        assert_eq!(regs.get(7), 700);

        // Switch to FIQ, r8-r12 are banked
        regs.switch_mode(CpuMode::Fiq);
        regs.set(8, 888);
        assert_eq!(regs.get(8), 888);

        regs.switch_mode(CpuMode::User);
        assert_eq!(regs.get(8), 0); // Not the FIQ banking
    }

    #[test]
    fn test_banked_r13_r14() {
        let mut regs = Registers::default();
        
        // User/System SP
        regs.switch_mode(CpuMode::User);
        regs.set(13, 0x1111);
        regs.set(14, 0x2222);

        // Supervisor SP
        regs.switch_mode(CpuMode::Supervisor);
        regs.set(13, 0x3333);
        regs.set(14, 0x4444);

        // Verify isolation
        regs.switch_mode(CpuMode::User);
        assert_eq!(regs.get(13), 0x1111);
        assert_eq!(regs.get(14), 0x2222);

        regs.switch_mode(CpuMode::Supervisor);
        assert_eq!(regs.get(13), 0x3333);
        assert_eq!(regs.get(14), 0x4444);
    }

    #[test]
    fn test_status_register_pack_unpack() {
        let mut sr = StatusRegister::new();
        sr.n = true;
        sr.z = false;
        sr.c = true;
        sr.v = false;
        sr.mode = CpuMode::Supervisor;

        let val = sr.to_u32();
        let mut sr2 = StatusRegister::default();
        sr2.from_u32(val);

        assert_eq!(sr2.n, true);
        assert_eq!(sr2.z, false);
        assert_eq!(sr2.c, true);
        assert_eq!(sr2.v, false);
        assert_eq!(sr2.mode, CpuMode::Supervisor);
    }

    #[test]
    fn test_pc_lookahead() {
        let mut cpu = Cpu::new();
        cpu.registers.r15 = 0x1000;
        
        cpu.registers.cpsr.t = false; // ARM mode
        assert_eq!(cpu.pc_lookahead(), 0x1008);

        cpu.registers.cpsr.t = true; // Thumb mode
        assert_eq!(cpu.pc_lookahead(), 0x1004);
    }

    #[test]
    fn test_hle_swi_cpu_set() {
        let mut cpu = Cpu::new();
        let mut bus = crate::bus::Bus::default();

        // Write some source data into IWRAM at 0x03001000
        bus.write_word(0x03001000, 0x11111111);
        bus.write_word(0x03001004, 0x22222222);

        // Setup R0 (src), R1 (dest), R2 (count/control)
        cpu.registers.set(0, 0x03001000);
        cpu.registers.set(1, 0x03002000);
        // Copy 2 words (32-bit copy, bit 26 set)
        cpu.registers.set(2, 2 | (1 << 26));

        // Call CpuSet HLE SWI
        cpu.handle_hle_swi(0x0B, &mut bus);

        // Assert copies occurred correctly
        assert_eq!(bus.read_word(0x03002000), 0x11111111);
        assert_eq!(bus.read_word(0x03002004), 0x22222222);
    }

    #[test]
    fn test_arm_alu_and_shifted_ops() {
        let mut cpu = Cpu::new();
        let mut bus = crate::bus::Bus::default();

        // 1. ARM MOV immediate: MOV R1, #0xFF (R1 = 255)
        // Format: cond=0xE (Always), I=1 (bit 25), opcode=0xD (MOV), S=0, Rn=0, Rd=1, rotate=0, imm=0xFF
        let mov_r1_255 = 0xE3A010FF;
        cpu.registers.r15 = 0x03000000;
        bus.write_word(0x03000000, mov_r1_255);
        let cycles = cpu.step(&mut bus);
        assert_eq!(cycles, 1);
        assert_eq!(cpu.registers.get(1), 255);
        assert_eq!(cpu.registers.r15, 0x03000004);

        // 2. ARM ADD with shifted register: ADD R2, R1, R1, LSL #2 (R2 = R1 + (R1 << 2) = 255 + 1020 = 1275)
        // Format: cond=0xE, I=0, opcode=0x4 (ADD), S=0, Rn=1, Rd=2, shift_imm=2, shift_type=0 (LSL), Rm=1
        let add_shifted = 0xE0812101;
        bus.write_word(0x03000004, add_shifted);
        let cycles = cpu.step(&mut bus);
        assert_eq!(cycles, 1);
        assert_eq!(cpu.registers.get(2), 1275);
        assert_eq!(cpu.registers.r15, 0x03000008);
    }

    #[test]
    fn test_arm_branch_ops() {
        let mut cpu = Cpu::new();
        let mut bus = crate::bus::Bus::default();
        cpu.registers.r15 = 0x03000000;

        // ARM Branch with Link: BL +8 (offset = 2 words)
        // Target PC = PC + 8 (lookahead) + (2 << 2) = 0x03000000 + 8 + 8 = 0x03000010
        // Link register R14 should be set to PC + 4 (instruction address + 4) = 0x03000004
        let bl_plus_8 = 0xEB000002;
        bus.write_word(0x03000000, bl_plus_8);
        
        let cycles = cpu.step(&mut bus);
        assert_eq!(cycles, 3); // Branch takes 3 cycles
        assert_eq!(cpu.registers.r15, 0x03000010);
        assert_eq!(cpu.registers.get(14), 0x03000004);
    }

    #[test]
    fn test_thumb_alu_ops() {
        let mut cpu = Cpu::new();
        let mut bus = crate::bus::Bus::default();
        cpu.registers.cpsr.t = true; // Switch to Thumb state
        cpu.registers.r15 = 0x03000000;

        // 1. Thumb MOV immediate: MOV R0, #42
        // Format Format 3: op=4 (MOV), Rd=0, imm=42 -> 0x202A
        bus.write_halfword(0x03000000, 0x202A);
        let cycles = cpu.step(&mut bus);
        assert_eq!(cycles, 1);
        assert_eq!(cpu.registers.get(0), 42);
        assert_eq!(cpu.registers.r15, 0x03000002);

        // 2. Thumb ADD register: ADD R1, R0, #5
        // Format Format 2: op=3 (Format 2), I=1 (bit 10), op=0 (ADD), Rd=1, Rs=0, offset3=5 -> 0x1D41
        bus.write_halfword(0x03000002, 0x1D41);
        let cycles = cpu.step(&mut bus);
        assert_eq!(cycles, 1);
        assert_eq!(cpu.registers.get(1), 47);
        assert_eq!(cpu.registers.r15, 0x03000004);
    }
}

