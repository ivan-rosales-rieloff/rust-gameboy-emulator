//!
//! # ARM Mode Instruction Decoder & Executors
//!
//! This module implements the 32-bit ARM instruction decoding, barrel shifter operations,
//! and standard execution paths for the ARM7TDMI CPU core.
//!

use crate::cpu::Cpu;
use crate::bus::Bus;

/// Barrel Shifter shift types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShiftType {
    Lsl = 0, // Logical Shift Left
    Lsr = 1, // Logical Shift Right
    Asr = 2, // Arithmetic Shift Right
    Ror = 3, // Rotate Right / RRX
}

impl Cpu {
    /// Evaluates if the standard ARM instruction condition passes based on CPSR flags.
    #[inline]
    pub fn check_condition(&self, cond: u8) -> bool {
        let cpsr = self.registers.cpsr;
        match cond {
            0x0 => cpsr.z,                          // EQ (Equal)
            0x1 => !cpsr.z,                         // NE (Not Equal)
            0x2 => cpsr.c,                          // CS / HS (Carry Set / Unsigned Higher or Same)
            0x3 => !cpsr.c,                         // CC / LO (Carry Clear / Unsigned Lower)
            0x4 => cpsr.n,                          // MI (Minus / Negative)
            0x5 => !cpsr.n,                         // PL (Plus / Positive or Zero)
            0x6 => cpsr.v,                          // VS (Overflow Set)
            0x7 => !cpsr.v,                         // VC (Overflow Clear)
            0x8 => cpsr.c && !cpsr.z,               // HI (Unsigned Higher)
            0x9 => !cpsr.c || cpsr.z,               // LS (Unsigned Lower or Same)
            0xA => cpsr.n == cpsr.v,                // GE (Greater than or Equal)
            0xB => cpsr.n != cpsr.v,                // LT (Less Than)
            0xC => !cpsr.z && (cpsr.n == cpsr.v),   // GT (Greater Than)
            0xD => cpsr.z || (cpsr.n != cpsr.v),    // LE (Less Than or Equal)
            0xE => true,                            // AL (Always)
            _ => true,                              // Reserved (Defaults to Always)
        }
    }

    /// Evaluates shifting logic on ROP2 (Operand 2) using the barrel shifter.
    /// Returns the shifted value and optionally the carry out bit.
    pub fn barrel_shift(&self, shift_type: ShiftType, amount: u32, value: u32, carry_in: bool) -> (u32, bool) {
        if amount == 0 {
            return (value, carry_in);
        }

        match shift_type {
            ShiftType::Lsl => {
                if amount >= 32 {
                    (0, if amount == 32 { value & 1 != 0 } else { false })
                } else {
                    (value << amount, (value & (1 << (32 - amount))) != 0)
                }
            }
            ShiftType::Lsr => {
                if amount >= 32 {
                    (0, if amount == 32 { (value >> 31) != 0 } else { false })
                } else {
                    (value >> amount, (value & (1 << (amount - 1))) != 0)
                }
            }
            ShiftType::Asr => {
                if amount >= 32 {
                    let sign = (value & 0x80000000) != 0;
                    (if sign { 0xFFFFFFFF } else { 0 }, sign)
                } else {
                    let shifted = ((value as i32) >> amount) as u32;
                    (shifted, (value & (1 << (amount - 1))) != 0)
                }
            }
            ShiftType::Ror => {
                let rot = amount % 32;
                if rot == 0 {
                    (value, carry_in)
                } else {
                    let shifted = (value >> rot) | (value << (32 - rot));
                    (shifted, (value & (1 << (rot - 1))) != 0)
                }
            }
        }
    }

    /// Decodes and executes a 32-bit ARM instruction. Returns cycle cost.
    pub fn execute_arm(&mut self, instruction: u32, bus: &mut Bus) -> u32 {
        let cond = (instruction >> 28) as u8;
        if !self.check_condition(cond) {
            return 1; // 1 cycle for ignored conditional instructions
        }

        let op1 = (instruction >> 25) & 7; // Bits 25-27: Category selector
        
        match op1 {
            0 | 1 => {
                // Data Processing or PSR Transfer
                self.execute_arm_data_processing(instruction, bus)
            }
            2 => {
                // Load/Store Immediate Offset
                self.execute_arm_ldr_str_immediate(instruction, bus)
            }
            5 => {
                // Branch or Branch with Link
                self.execute_arm_branch(instruction, bus)
            }
            _ => {
                // Unimplemented category, treat as 1-cycle NOP
                1
            }
        }
    }

    fn execute_arm_branch(&mut self, instruction: u32, _bus: &mut Bus) -> u32 {
        let link = (instruction & (1 << 24)) != 0;
        let mut offset = instruction & 0x00FFFFFF;

        // Sign extend the 24-bit offset to 32 bits
        if (offset & 0x00800000) != 0 {
            offset |= 0xFF000000;
        }

        // Branch offset is shifted left by 2 bytes (word aligned)
        let target = self.registers.r15.wrapping_add(8).wrapping_add(offset << 2);

        if link {
            let lr = self.registers.r15.wrapping_add(4); // PC after branch instruction
            self.registers.set(14, lr); // Set LR (R14)
        }

        self.registers.set(15, target); // Branch PC update
        3 // Branches consume 3 cycles (pipeline reload penalty)
    }

    fn execute_arm_data_processing(&mut self, instruction: u32, _bus: &mut Bus) -> u32 {
        let is_immediate = (instruction & (1 << 25)) != 0;
        let opcode = ((instruction >> 21) & 0xF) as u8;
        let set_flags = (instruction & (1 << 20)) != 0;
        let rn = ((instruction >> 16) & 0xF) as u8;
        let rd = ((instruction >> 12) & 0xF) as u8;

        let rn_val = if rn == 15 {
            self.registers.r15.wrapping_add(8) // Read PC standard pipeline offset
        } else {
            self.registers.get(rn)
        };

        // Determine Operand 2
        let (op2, carry_out) = if is_immediate {
            let rotate = ((instruction >> 8) & 0xF) * 2;
            let val = instruction & 0xFF;
            let rot = rotate % 32;
            if rot == 0 {
                (val, self.registers.cpsr.c)
            } else {
                let shifted = (val >> rot) | (val << (32 - rot));
                (shifted, (val & (1 << (rot - 1))) != 0)
            }
        } else {
            let rm = (instruction & 0xF) as u8;
            let rm_val = self.registers.get(rm);
            let shift_by_reg = (instruction & (1 << 4)) != 0;
            let shift_t = match (instruction >> 5) & 3 {
                0 => ShiftType::Lsl,
                1 => ShiftType::Lsr,
                2 => ShiftType::Asr,
                _ => ShiftType::Ror,
            };

            let amount = if shift_by_reg {
                let rs = ((instruction >> 8) & 0xF) as u8;
                self.registers.get(rs) & 0xFF
            } else {
                (instruction >> 7) & 0x1F
            };

            self.barrel_shift(shift_t, amount, rm_val, self.registers.cpsr.c)
        };

        let mut result = 0u32;
        let mut carry = self.registers.cpsr.c;
        let mut overflow = self.registers.cpsr.v;

        match opcode {
            0x0 => { // AND
                result = rn_val & op2;
                carry = carry_out;
            }
            0x1 => { // EOR
                result = rn_val ^ op2;
                carry = carry_out;
            }
            0x2 => { // SUB
                let (res, c) = rn_val.overflowing_sub(op2);
                result = res;
                carry = !c;
                let (_, overflow_bit) = (rn_val as i32).overflowing_sub(op2 as i32);
                overflow = overflow_bit;
            }
            0x4 => { // ADD
                let (res, c) = rn_val.overflowing_add(op2);
                result = res;
                carry = c;
                let (_, overflow_bit) = (rn_val as i32).overflowing_add(op2 as i32);
                overflow = overflow_bit;
            }
            0x8 => { // TST
                result = rn_val & op2;
                carry = carry_out;
            }
            0x9 => { // TEQ
                result = rn_val ^ op2;
                carry = carry_out;
            }
            0xA => { // CMP
                let (res, c) = rn_val.overflowing_sub(op2);
                result = res;
                carry = !c;
                let (_, overflow_bit) = (rn_val as i32).overflowing_sub(op2 as i32);
                overflow = overflow_bit;
            }
            0xC => { // ORR
                result = rn_val | op2;
                carry = carry_out;
            }
            0xD => { // MOV
                result = op2;
                carry = carry_out;
            }
            0xE => { // BIC
                result = rn_val & !op2;
                carry = carry_out;
            }
            _ => {}
        }

        // Writes result if opcode is not a comparison/test
        let is_comp = opcode == 0x8 || opcode == 0x9 || opcode == 0xA;
        if !is_comp {
            self.registers.set(rd, result);
        }

        if set_flags {
            self.registers.cpsr.n = (result & 0x80000000) != 0;
            self.registers.cpsr.z = result == 0;
            self.registers.cpsr.c = carry;
            self.registers.cpsr.v = overflow;
        }

        if rd == 15 {
            3 // Target PC modified: trigger pipeline cycle penalty
        } else {
            1
        }
    }

    fn execute_arm_ldr_str_immediate(&mut self, instruction: u32, bus: &mut Bus) -> u32 {
        let is_load = (instruction & (1 << 20)) != 0;
        let writeback = (instruction & (1 << 21)) != 0;
        let is_byte = (instruction & (1 << 22)) != 0;
        let add_offset = (instruction & (1 << 23)) != 0;
        let pre_index = (instruction & (1 << 24)) != 0;
        let rn = ((instruction >> 16) & 0xF) as u8;
        let rd = ((instruction >> 12) & 0xF) as u8;
        let offset = instruction & 0xFFF;

        let rn_val = if rn == 15 {
            self.registers.r15.wrapping_add(8)
        } else {
            self.registers.get(rn)
        };

        let calculated_offset = if add_offset { offset } else { 0u32.wrapping_sub(offset) };
        let target_addr = if pre_index {
            rn_val.wrapping_add(calculated_offset)
        } else {
            rn_val
        };

        let mut cycles = 1;

        if is_load {
            let data = if is_byte {
                bus.read_byte(target_addr) as u32
            } else {
                bus.read_word(target_addr)
            };
            self.registers.set(rd, data);
            cycles += 1; // Loads take extra cycle to read memory
            if rd == 15 {
                cycles += 2; // Extra pipeline reload penalty
            }
        } else {
            let data = self.registers.get(rd);
            if is_byte {
                bus.write_byte(target_addr, data as u8);
            } else {
                bus.write_word(target_addr, data);
            }
        }

        // Apply post-index writebacks
        if !pre_index || writeback {
            let next_base = rn_val.wrapping_add(calculated_offset);
            self.registers.set(rn, next_base);
        }

        cycles
    }
}
