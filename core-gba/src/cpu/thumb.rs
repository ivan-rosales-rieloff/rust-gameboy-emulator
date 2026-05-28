//!
//! # Thumb Mode Instruction Decoder & Executors
//!
//! This module implements the 16-bit Thumb instruction decoding and standard execution paths
//! for the ARM7TDMI CPU core.
//!

use crate::cpu::Cpu;
use crate::bus::Bus;

impl Cpu {
    /// Decodes and executes a 16-bit Thumb instruction. Returns cycle cost.
    pub fn execute_thumb(&mut self, instruction: u16, bus: &mut Bus) -> u32 {
        let op = (instruction >> 11) as u8;

        match op {
            0..=3 => {
                // Format 1 & 2: Shift or Add/Subtract
                self.execute_thumb_shift_add_sub(instruction)
            }
            4..=7 => {
                // Format 3: Move/Compare/Add/Subtract immediate
                self.execute_thumb_immediate_alu(instruction)
            }
            8 => {
                // Format 4: ALU Operations
                self.execute_thumb_alu_ops(instruction)
            }
            9 => {
                // Format 5: Hi Register / Branch Exchange
                self.execute_thumb_hi_reg_ops(instruction)
            }
            10..=11 => {
                // Format 6 & 7: PC-Relative Load or Load/Store Offset
                self.execute_thumb_load_store_offset(instruction, bus)
            }
            24..=25 => {
                // Format 12 & 13: Load Address or SP Add/Sub
                self.execute_thumb_sp_pc_math(instruction)
            }
            28 => {
                // Format 16: Conditional Branch
                self.execute_thumb_cond_branch(instruction)
            }
            29 => {
                // Format 17: SWI
                let swi_number = (instruction & 0xFF) as u8;
                self.handle_hle_swi(swi_number, bus);
                1
            }
            30 => {
                // Format 18: Unconditional Branch
                let mut offset = (instruction & 0x7FF) as u32;
                if (offset & 0x400) != 0 {
                    offset |= 0xFFFFF800; // Sign-extend
                }
                let target = self.registers.r15.wrapping_add(4).wrapping_add(offset << 1);
                self.registers.set(15, target);
                3 // 3 cycles branch penalty
            }
            31 => {
                // Format 19: Long Branch with Link
                self.execute_thumb_long_branch(instruction)
            }
            _ => {
                // Unimplemented Thumb opcode: treat as 1-cycle NOP
                1
            }
        }
    }

    fn execute_thumb_shift_add_sub(&mut self, instruction: u16) -> u32 {
        let op = (instruction >> 11) as u8;
        let rd = (instruction & 7) as u8;
        let rs = ((instruction >> 3) & 7) as u8;
        let rs_val = self.registers.get(rs);

        if op == 3 {
            // Format 2: Add/Subtract
            let is_imm = (instruction & (1 << 10)) != 0;
            let is_sub = (instruction & (1 << 9)) != 0;
            let offset3 = ((instruction >> 6) & 7) as u32;
            let val = if is_imm { offset3 } else { self.registers.get(offset3 as u8) };

            if is_sub {
                let (res, c) = rs_val.overflowing_sub(val);
                self.registers.set(rd, res);
                self.registers.cpsr.n = (res & 0x80000000) != 0;
                self.registers.cpsr.z = res == 0;
                self.registers.cpsr.c = !c;
                let (_, overflow) = (rs_val as i32).overflowing_sub(val as i32);
                self.registers.cpsr.v = overflow;
            } else {
                let (res, c) = rs_val.overflowing_add(val);
                self.registers.set(rd, res);
                self.registers.cpsr.n = (res & 0x80000000) != 0;
                self.registers.cpsr.z = res == 0;
                self.registers.cpsr.c = c;
                let (_, overflow) = (rs_val as i32).overflowing_add(val as i32);
                self.registers.cpsr.v = overflow;
            }
        } else {
            // Format 1: Move shifted register
            let offset5 = ((instruction >> 6) & 0x1F) as u32;
            match op {
                0 => { // LSL
                    let (res, carry) = self.barrel_shift(super::arm::ShiftType::Lsl, offset5, rs_val, self.registers.cpsr.c);
                    self.registers.set(rd, res);
                    self.registers.cpsr.n = (res & 0x80000000) != 0;
                    self.registers.cpsr.z = res == 0;
                    self.registers.cpsr.c = carry;
                }
                1 => { // LSR
                    let (res, carry) = self.barrel_shift(super::arm::ShiftType::Lsr, offset5, rs_val, self.registers.cpsr.c);
                    self.registers.set(rd, res);
                    self.registers.cpsr.n = (res & 0x80000000) != 0;
                    self.registers.cpsr.z = res == 0;
                    self.registers.cpsr.c = carry;
                }
                2 => { // ASR
                    let (res, carry) = self.barrel_shift(super::arm::ShiftType::Asr, offset5, rs_val, self.registers.cpsr.c);
                    self.registers.set(rd, res);
                    self.registers.cpsr.n = (res & 0x80000000) != 0;
                    self.registers.cpsr.z = res == 0;
                    self.registers.cpsr.c = carry;
                }
                _ => {}
            }
        }
        1
    }

    fn execute_thumb_immediate_alu(&mut self, instruction: u16) -> u32 {
        let op_format = (instruction >> 11) & 3;
        let rd = ((instruction >> 8) & 7) as u8;
        let imm = (instruction & 0xFF) as u32;

        let rd_val = self.registers.get(rd);

        match op_format {
            0 => { // MOV
                self.registers.set(rd, imm);
                self.registers.cpsr.n = (imm & 0x80000000) != 0;
                self.registers.cpsr.z = imm == 0;
            }
            1 => { // CMP
                let (res, c) = rd_val.overflowing_sub(imm);
                self.registers.cpsr.n = (res & 0x80000000) != 0;
                self.registers.cpsr.z = res == 0;
                self.registers.cpsr.c = !c;
                let (_, overflow) = (rd_val as i32).overflowing_sub(imm as i32);
                self.registers.cpsr.v = overflow;
            }
            2 => { // ADD
                let (res, c) = rd_val.overflowing_add(imm);
                self.registers.set(rd, res);
                self.registers.cpsr.n = (res & 0x80000000) != 0;
                self.registers.cpsr.z = res == 0;
                self.registers.cpsr.c = c;
                let (_, overflow) = (rd_val as i32).overflowing_add(imm as i32);
                self.registers.cpsr.v = overflow;
            }
            3 => { // SUB
                let (res, c) = rd_val.overflowing_sub(imm);
                self.registers.set(rd, res);
                self.registers.cpsr.n = (res & 0x80000000) != 0;
                self.registers.cpsr.z = res == 0;
                self.registers.cpsr.c = !c;
                let (_, overflow) = (rd_val as i32).overflowing_sub(imm as i32);
                self.registers.cpsr.v = overflow;
            }
            _ => {}
        }
        1
    }

    fn execute_thumb_alu_ops(&mut self, instruction: u16) -> u32 {
        let opcode = ((instruction >> 6) & 0xF) as u8;
        let rs = ((instruction >> 3) & 7) as u8;
        let rd = (instruction & 7) as u8;

        let rd_val = self.registers.get(rd);
        let rs_val = self.registers.get(rs);

        let mut result = 0u32;
        let mut carry = self.registers.cpsr.c;
        let mut overflow = self.registers.cpsr.v;

        match opcode {
            0x0 => { // AND
                result = rd_val & rs_val;
            }
            0x1 => { // EOR
                result = rd_val ^ rs_val;
            }
            0x2 => { // LSL
                let (res, c) = self.barrel_shift(super::arm::ShiftType::Lsl, rs_val, rd_val, carry);
                result = res;
                carry = c;
            }
            0x3 => { // LSR
                let (res, c) = self.barrel_shift(super::arm::ShiftType::Lsr, rs_val, rd_val, carry);
                result = res;
                carry = c;
            }
            0x4 => { // ASR
                let (res, c) = self.barrel_shift(super::arm::ShiftType::Asr, rs_val, rd_val, carry);
                result = res;
                carry = c;
            }
            0x5 => { // ADC
                let c_val = if carry { 1 } else { 0 };
                let (res1, c1) = rd_val.overflowing_add(rs_val);
                let (res2, c2) = res1.overflowing_add(c_val);
                result = res2;
                carry = c1 || c2;
                let (_, overflow_bit) = (rd_val as i32).overflowing_add(rs_val as i32);
                overflow = overflow_bit;
            }
            0x6 => { // SBC
                let c_val = if carry { 0 } else { 1 };
                let (res1, c1) = rd_val.overflowing_sub(rs_val);
                let (res2, c2) = res1.overflowing_sub(c_val);
                result = res2;
                carry = !(c1 || c2);
                let (_, overflow_bit) = (rd_val as i32).overflowing_sub(rs_val as i32);
                overflow = overflow_bit;
            }
            0x7 => { // ROR
                let (res, c) = self.barrel_shift(super::arm::ShiftType::Ror, rs_val, rd_val, carry);
                result = res;
                carry = c;
            }
            0x8 => { // TST
                result = rd_val & rs_val;
            }
            0x9 => { // NEG
                let (res, c) = 0u32.overflowing_sub(rs_val);
                result = res;
                carry = !c;
                let (_, overflow_bit) = 0i32.overflowing_sub(rs_val as i32);
                overflow = overflow_bit;
            }
            0xA => { // CMP
                let (res, c) = rd_val.overflowing_sub(rs_val);
                result = res;
                carry = !c;
                let (_, overflow_bit) = (rd_val as i32).overflowing_sub(rs_val as i32);
                overflow = overflow_bit;
            }
            0xC => { // ORR
                result = rd_val | rs_val;
            }
            0xD => { // MUL
                result = rd_val.wrapping_mul(rs_val);
                carry = false; // Multiplications modify carry unpredictably, reset
            }
            0xE => { // BIC
                result = rd_val & !rs_val;
            }
            0xF => { // MVN
                result = !rs_val;
            }
            _ => {}
        }

        let is_comp = opcode == 0x8 || opcode == 0xA;
        if !is_comp {
            self.registers.set(rd, result);
        }

        self.registers.cpsr.n = (result & 0x80000000) != 0;
        self.registers.cpsr.z = result == 0;
        self.registers.cpsr.c = carry;
        self.registers.cpsr.v = overflow;
        1
    }

    fn execute_thumb_hi_reg_ops(&mut self, instruction: u16) -> u32 {
        let opcode = ((instruction >> 8) & 3) as u8;
        let h1 = (instruction & (1 << 7)) != 0;
        let h2 = (instruction & (1 << 6)) != 0;
        let rs = (((instruction >> 3) & 7) | (if h2 { 8 } else { 0 })) as u8;
        let rd = ((instruction & 7) | (if h1 { 8 } else { 0 })) as u8;

        let rd_val = self.registers.get(rd);
        let rs_val = self.registers.get(rs);

        match opcode {
            0 => { // ADD
                let target = rd_val.wrapping_add(rs_val);
                self.registers.set(rd, target);
            }
            1 => { // CMP
                let res = rd_val.wrapping_sub(rs_val);
                self.registers.cpsr.n = (res & 0x80000000) != 0;
                self.registers.cpsr.z = res == 0;
                self.registers.cpsr.c = rd_val >= rs_val;
                let (_, overflow) = (rd_val as i32).overflowing_sub(rs_val as i32);
                self.registers.cpsr.v = overflow;
            }
            2 => { // MOV
                self.registers.set(rd, rs_val);
            }
            3 => { // BX
                let target = rs_val;
                self.registers.cpsr.t = (target & 1) != 0;
                self.registers.set(15, target & !1);
                return 3;
            }
            _ => {}
        }
        
        if rd == 15 {
            3
        } else {
            1
        }
    }

    fn execute_thumb_load_store_offset(&mut self, instruction: u16, bus: &mut Bus) -> u32 {
        let is_load = (instruction & (1 << 11)) != 0;
        let is_byte = (instruction & (1 << 10)) != 0;
        let offset = ((instruction >> 6) & 0x1F) as u32;
        let rs = ((instruction >> 3) & 7) as u8;
        let rd = (instruction & 7) as u8;

        let base = self.registers.get(rs);
        let target_addr = if is_byte {
            base.wrapping_add(offset)
        } else {
            base.wrapping_add(offset << 2) // Word offsets shifted
        };

        if is_load {
            let data = if is_byte {
                bus.read_byte(target_addr) as u32
            } else {
                bus.read_word(target_addr)
            };
            self.registers.set(rd, data);
            2 // Loads take extra cycle
        } else {
            let data = self.registers.get(rd);
            if is_byte {
                bus.write_byte(target_addr, data as u8);
            } else {
                bus.write_word(target_addr, data);
            }
            1
        }
    }

    fn execute_thumb_sp_pc_math(&mut self, instruction: u16) -> u32 {
        let use_sp = (instruction & (1 << 11)) != 0;
        let rd = ((instruction >> 8) & 7) as u8;
        let imm = ((instruction & 0xFF) << 2) as u32;

        let base = if use_sp {
            self.registers.get(13) // SP
        } else {
            self.registers.r15.wrapping_add(4) // PC lookahead for Thumb math
        };

        self.registers.set(rd, base.wrapping_add(imm));
        1
    }

    fn execute_thumb_cond_branch(&mut self, instruction: u16) -> u32 {
        let cond = ((instruction >> 8) & 0xF) as u8;
        let mut offset = (instruction & 0xFF) as u32;

        if (offset & 0x80) != 0 {
            offset |= 0xFFFFFF00; // Sign-extend
        }

        if self.check_condition(cond) {
            let target = self.registers.r15.wrapping_add(4).wrapping_add(offset << 1);
            self.registers.set(15, target);
            3 // 3 cycles branch penalty
        } else {
            1
        }
    }

    fn execute_thumb_long_branch(&mut self, instruction: u16) -> u32 {
        let bit11 = (instruction & (1 << 11)) != 0;
        let offset = (instruction & 0x7FF) as u32;

        if !bit11 {
            // First instruction: offset high
            let mut val = offset << 12;
            if (val & (1 << 22)) != 0 {
                val |= 0xFF800000;
            }
            let lr = self.registers.r15.wrapping_add(4).wrapping_add(val);
            self.registers.set(14, lr);
        } else {
            // Second instruction: offset low
            let target = self.registers.get(14).wrapping_add(offset << 1);
            let next_lr = self.registers.r15.wrapping_add(2) | 1;
            self.registers.set(15, target);
            self.registers.set(14, next_lr);
        }
        3
    }
}
