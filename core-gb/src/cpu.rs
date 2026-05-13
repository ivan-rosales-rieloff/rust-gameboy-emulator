use std::error::Error;
use std::fmt::{Display, Formatter};

use core_common::StepResult;

use crate::bus::Bus;

const FLAG_Z: u8 = 0x80;
const FLAG_N: u8 = 0x40;
const FLAG_H: u8 = 0x20;
const FLAG_C: u8 = 0x10;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Registers {
    pub a: u8,
    pub f: u8,
    pub b: u8,
    pub c: u8,
    pub d: u8,
    pub e: u8,
    pub h: u8,
    pub l: u8,
    pub sp: u16,
    pub pc: u16,
}

impl Default for Registers {
    fn default() -> Self {
        Self {
            a: 0x01,
            f: 0xB0,
            b: 0x00,
            c: 0x13,
            d: 0x00,
            e: 0xD8,
            h: 0x01,
            l: 0x4D,
            sp: 0xFFFE,
            pc: 0x0100,
        }
    }
}

#[derive(Debug, Default)]
pub struct Cpu {
    registers: Registers,
    halted: bool,
    ime: bool, // Interrupt Master Enable flag
}

impl Cpu {
    pub fn pc(&self) -> u16 {
        self.registers.pc
    }

    pub fn registers(&self) -> Registers {
        self.registers
    }

    fn bc(&self) -> u16 {
        u16::from(self.registers.b) << 8 | u16::from(self.registers.c)
    }

    fn de(&self) -> u16 {
        u16::from(self.registers.d) << 8 | u16::from(self.registers.e)
    }

    fn hl(&self) -> u16 {
        u16::from(self.registers.h) << 8 | u16::from(self.registers.l)
    }

    fn set_hl(&mut self, value: u16) {
        self.registers.h = (value >> 8) as u8;
        self.registers.l = value as u8;
    }

    fn set_sp(&mut self, value: u16) {
        self.registers.sp = value;
    }

    fn set_flag(&mut self, mask: u8, enabled: bool) {
        if enabled {
            self.registers.f |= mask;
        } else {
            self.registers.f &= !mask;
        }
    }

    fn update_flags(&mut self, z: bool, n: bool, h: bool, c: bool) {
        self.registers.f = 0;
        self.set_flag(FLAG_Z, z);
        self.set_flag(FLAG_N, n);
        self.set_flag(FLAG_H, h);
        self.set_flag(FLAG_C, c);
    }

    fn read_reg8(&self, code: u8, bus: &mut Bus) -> u8 {
        match code {
            0 => self.registers.b,
            1 => self.registers.c,
            2 => self.registers.d,
            3 => self.registers.e,
            4 => self.registers.h,
            5 => self.registers.l,
            6 => bus.read8(self.hl()),
            7 => self.registers.a,
            _ => unreachable!(),
        }
    }

    fn write_reg8(&mut self, code: u8, value: u8, bus: &mut Bus) {
        match code {
            0 => self.registers.b = value,
            1 => self.registers.c = value,
            2 => self.registers.d = value,
            3 => self.registers.e = value,
            4 => self.registers.h = value,
            5 => self.registers.l = value,
            6 => bus.write8(self.hl(), value),
            7 => self.registers.a = value,
            _ => unreachable!(),
        }
    }

    fn add_a(&mut self, value: u8) {
        let a = self.registers.a;
        let (result, carry) = a.overflowing_add(value);
        let half = (a & 0x0F) + (value & 0x0F) > 0x0F;
        self.registers.a = result;
        self.update_flags(result == 0, false, half, carry);
    }

    fn sub_a(&mut self, value: u8) {
        let a = self.registers.a;
        let (result, borrow) = a.overflowing_sub(value);
        let half = (a & 0x0F) < (value & 0x0F);
        self.registers.a = result;
        self.update_flags(result == 0, true, half, borrow);
    }

    fn and_a(&mut self, value: u8) {
        let result = self.registers.a & value;
        self.registers.a = result;
        self.update_flags(result == 0, false, true, false);
    }

    fn xor_a(&mut self, value: u8) {
        let result = self.registers.a ^ value;
        self.registers.a = result;
        self.update_flags(result == 0, false, false, false);
    }

    fn or_a(&mut self, value: u8) {
        let result = self.registers.a | value;
        self.registers.a = result;
        self.update_flags(result == 0, false, false, false);
    }

    fn cp_a(&mut self, value: u8) {
        let a = self.registers.a;
        let (result, borrow) = a.overflowing_sub(value);
        let half = (a & 0x0F) < (value & 0x0F);
        self.update_flags(result == 0, true, half, borrow);
    }

    fn inc8(&mut self, value: u8) -> u8 {
        let result = value.wrapping_add(1);
        let half = (value & 0x0F) == 0x0F;
        self.update_flags(result == 0, false, half, self.registers.f & FLAG_C != 0);
        result
    }

    fn dec8(&mut self, value: u8) -> u8 {
        let result = value.wrapping_sub(1);
        let half = (value & 0x0F) == 0x00;
        self.update_flags(result == 0, true, half, self.registers.f & FLAG_C != 0);
        result
    }

    fn fetch8(&mut self, bus: &mut Bus) -> u8 {
        let byte = bus.read8(self.registers.pc);
        self.registers.pc = self.registers.pc.wrapping_add(1);
        byte
    }

    fn fetch16(&mut self, bus: &mut Bus) -> u16 {
        let low = u16::from(self.fetch8(bus));
        let high = u16::from(self.fetch8(bus));
        low | (high << 8)
    }

    pub fn step(&mut self, bus: &mut Bus) -> Result<StepResult, CpuError> {
        if self.halted {
            return Ok(StepResult::new(4, true));
        }

        let instruction_address = self.registers.pc;
        let opcode = self.fetch8(bus);

        let step_result = match opcode {
            0x00 => StepResult::new(4, false),
            0x04..=0x0F if opcode & 0x07 == 0x04 => {
                let reg = (opcode >> 3) & 0x07;
                let value = self.read_reg8(reg, bus);
                let updated = self.inc8(value);
                self.write_reg8(reg, updated, bus);
                StepResult::new(if reg == 6 { 12 } else { 4 }, false)
            }
            0x05..=0x0F if opcode & 0x07 == 0x05 => {
                let reg = (opcode >> 3) & 0x07;
                let value = self.read_reg8(reg, bus);
                let updated = self.dec8(value);
                self.write_reg8(reg, updated, bus);
                StepResult::new(if reg == 6 { 12 } else { 4 }, false)
            }
            0x06 => {
                self.registers.b = self.fetch8(bus);
                StepResult::new(8, false)
            }
            0x0E => {
                self.registers.c = self.fetch8(bus);
                StepResult::new(8, false)
            }
            0x16 => {
                self.registers.d = self.fetch8(bus);
                StepResult::new(8, false)
            }
            0x1E => {
                self.registers.e = self.fetch8(bus);
                StepResult::new(8, false)
            }
            0x26 => {
                self.registers.h = self.fetch8(bus);
                StepResult::new(8, false)
            }
            0x2E => {
                self.registers.l = self.fetch8(bus);
                StepResult::new(8, false)
            }
            0x3E => {
                self.registers.a = self.fetch8(bus);
                StepResult::new(8, false)
            }
            0x3C => {
                // INC A
                self.registers.a = self.inc8(self.registers.a);
                StepResult::new(4, false)
            }
            0x02 => {
                bus.write8(self.bc(), self.registers.a);
                StepResult::new(8, false)
            }
            0x12 => {
                bus.write8(self.de(), self.registers.a);
                StepResult::new(8, false)
            }
            0x22 => {
                bus.write8(self.hl(), self.registers.a);
                self.set_hl(self.hl().wrapping_add(1));
                StepResult::new(8, false)
            }
            0x32 => {
                bus.write8(self.hl(), self.registers.a);
                self.set_hl(self.hl().wrapping_sub(1));
                StepResult::new(8, false)
            }
            0x0A => {
                self.registers.a = bus.read8(self.bc());
                StepResult::new(8, false)
            }
            0x1A => {
                self.registers.a = bus.read8(self.de());
                StepResult::new(8, false)
            }
            0x2A => {
                self.registers.a = bus.read8(self.hl());
                self.set_hl(self.hl().wrapping_add(1));
                StepResult::new(8, false)
            }
            0x3A => {
                self.registers.a = bus.read8(self.hl());
                self.set_hl(self.hl().wrapping_sub(1));
                StepResult::new(8, false)
            }
            0x21 => {
                let value = self.fetch16(bus);
                self.set_hl(value);
                StepResult::new(12, false)
            }
            0x01 => {
                // LD BC,nn
                let value = self.fetch16(bus);
                self.registers.b = (value >> 8) as u8;
                self.registers.c = value as u8;
                StepResult::new(12, false)
            }
            0x11 => {
                // LD DE,nn
                let value = self.fetch16(bus);
                self.registers.d = (value >> 8) as u8;
                self.registers.e = value as u8;
                StepResult::new(12, false)
            }
            0x03 => {
                // INC BC
                let bc = self.bc().wrapping_add(1);
                self.registers.b = (bc >> 8) as u8;
                self.registers.c = bc as u8;
                StepResult::new(8, false)
            }
            0x13 => {
                // INC DE
                let de = self.de().wrapping_add(1);
                self.registers.d = (de >> 8) as u8;
                self.registers.e = de as u8;
                StepResult::new(8, false)
            }
            0x23 => {
                // INC HL
                let hl = self.hl().wrapping_add(1);
                self.set_hl(hl);
                StepResult::new(8, false)
            }
            0x33 => {
                // INC SP
                self.registers.sp = self.registers.sp.wrapping_add(1);
                StepResult::new(8, false)
            }
            0x31 => {
                let value = self.fetch16(bus);
                self.set_sp(value);
                StepResult::new(12, false)
            }
            0x0B => {
                // DEC BC
                let bc = self.bc().wrapping_sub(1);
                self.registers.b = (bc >> 8) as u8;
                self.registers.c = bc as u8;
                StepResult::new(8, false)
            }
            0x1B => {
                // DEC DE
                let de = self.de().wrapping_sub(1);
                self.registers.d = (de >> 8) as u8;
                self.registers.e = de as u8;
                StepResult::new(8, false)
            }
            0x2B => {
                // DEC HL
                let hl = self.hl().wrapping_sub(1);
                self.set_hl(hl);
                StepResult::new(8, false)
            }
            0x3B => {
                // DEC SP
                self.registers.sp = self.registers.sp.wrapping_sub(1);
                StepResult::new(8, false)
            }
            0x76 => {
                self.halted = true;
                StepResult::new(4, true)
            }
            0x40..=0x7F if opcode != 0x76 => {
                let destination = (opcode - 0x40) >> 3;
                let source = opcode & 0x07;
                let value = self.read_reg8(source, bus);
                self.write_reg8(destination, value, bus);
                StepResult::new(if destination == 6 || source == 6 { 8 } else { 4 }, false)
            }
            0x80..=0x87 => {
                let value = self.read_reg8(opcode & 0x07, bus);
                self.add_a(value);
                StepResult::new(4, false)
            }
            0x90..=0x97 => {
                let value = self.read_reg8(opcode & 0x07, bus);
                self.sub_a(value);
                StepResult::new(4, false)
            }
            0xA0..=0xA7 => {
                let value = self.read_reg8(opcode & 0x07, bus);
                self.and_a(value);
                StepResult::new(4, false)
            }
            0xA8..=0xAF => {
                let value = self.read_reg8(opcode & 0x07, bus);
                self.xor_a(value);
                StepResult::new(4, false)
            }
            0xB0..=0xB7 => {
                let value = self.read_reg8(opcode & 0x07, bus);
                self.or_a(value);
                StepResult::new(4, false)
            }
            0xB8..=0xBF => {
                let value = self.read_reg8(opcode & 0x07, bus);
                self.cp_a(value);
                StepResult::new(4, false)
            }
            0x18 => {
                let offset = self.fetch8(bus) as i8;
                self.registers.pc = self.registers.pc.wrapping_add(offset as u16);
                StepResult::new(12, false)
            }
            0x20 => {
                // JR NZ - Jump relative if not zero
                let offset = self.fetch8(bus) as i8;
                if self.registers.f & FLAG_Z == 0 {
                    self.registers.pc = self.registers.pc.wrapping_add(offset as u16);
                    StepResult::new(12, false)
                } else {
                    StepResult::new(8, false)
                }
            }
            0x28 => {
                // JR Z - Jump relative if zero
                let offset = self.fetch8(bus) as i8;
                if self.registers.f & FLAG_Z != 0 {
                    self.registers.pc = self.registers.pc.wrapping_add(offset as u16);
                    StepResult::new(12, false)
                } else {
                    StepResult::new(8, false)
                }
            }
            0x30 => {
                // JR NC - Jump relative if not carry
                let offset = self.fetch8(bus) as i8;
                if self.registers.f & FLAG_C == 0 {
                    self.registers.pc = self.registers.pc.wrapping_add(offset as u16);
                    StepResult::new(12, false)
                } else {
                    StepResult::new(8, false)
                }
            }
            0x38 => {
                // JR C - Jump relative if carry
                let offset = self.fetch8(bus) as i8;
                if self.registers.f & FLAG_C != 0 {
                    self.registers.pc = self.registers.pc.wrapping_add(offset as u16);
                    StepResult::new(12, false)
                } else {
                    StepResult::new(8, false)
                }
            }
            0xC3 => {
                let address = self.fetch16(bus);
                self.registers.pc = address;
                StepResult::new(16, false)
            }
            0xC2 => {
                // JP NZ - Jump if not zero
                let address = self.fetch16(bus);
                if self.registers.f & FLAG_Z == 0 {
                    self.registers.pc = address;
                    StepResult::new(16, false)
                } else {
                    StepResult::new(12, false)
                }
            }
            0xCA => {
                // JP Z - Jump if zero
                let address = self.fetch16(bus);
                if self.registers.f & FLAG_Z != 0 {
                    self.registers.pc = address;
                    StepResult::new(16, false)
                } else {
                    StepResult::new(12, false)
                }
            }
            0xD2 => {
                // JP NC - Jump if not carry
                let address = self.fetch16(bus);
                if self.registers.f & FLAG_C == 0 {
                    self.registers.pc = address;
                    StepResult::new(16, false)
                } else {
                    StepResult::new(12, false)
                }
            }
            0xDA => {
                // JP C - Jump if carry
                let address = self.fetch16(bus);
                if self.registers.f & FLAG_C != 0 {
                    self.registers.pc = address;
                    StepResult::new(16, false)
                } else {
                    StepResult::new(12, false)
                }
            }
            0xCD => {
                let address = self.fetch16(bus);
                let return_address = self.registers.pc;
                self.registers.sp = self.registers.sp.wrapping_sub(2);
                bus.write8(self.registers.sp, (return_address & 0xFF) as u8);
                bus.write8(self.registers.sp.wrapping_add(1), (return_address >> 8) as u8);
                self.registers.pc = address;
                StepResult::new(24, false)
            }
            0xC9 => {
                let low = bus.read8(self.registers.sp);
                let high = bus.read8(self.registers.sp.wrapping_add(1));
                self.registers.sp = self.registers.sp.wrapping_add(2);
                self.registers.pc = u16::from(low) | (u16::from(high) << 8);
                StepResult::new(16, false)
            }
            0xC0 => {
                // RET NZ - Return if not zero
                if self.registers.f & FLAG_Z == 0 {
                    let low = bus.read8(self.registers.sp);
                    let high = bus.read8(self.registers.sp.wrapping_add(1));
                    self.registers.sp = self.registers.sp.wrapping_add(2);
                    self.registers.pc = u16::from(low) | (u16::from(high) << 8);
                    StepResult::new(20, false)
                } else {
                    StepResult::new(8, false)
                }
            }
            0xC8 => {
                // RET Z - Return if zero
                if self.registers.f & FLAG_Z != 0 {
                    let low = bus.read8(self.registers.sp);
                    let high = bus.read8(self.registers.sp.wrapping_add(1));
                    self.registers.sp = self.registers.sp.wrapping_add(2);
                    self.registers.pc = u16::from(low) | (u16::from(high) << 8);
                    StepResult::new(20, false)
                } else {
                    StepResult::new(8, false)
                }
            }
            0xD0 => {
                // RET NC - Return if not carry
                if self.registers.f & FLAG_C == 0 {
                    let low = bus.read8(self.registers.sp);
                    let high = bus.read8(self.registers.sp.wrapping_add(1));
                    self.registers.sp = self.registers.sp.wrapping_add(2);
                    self.registers.pc = u16::from(low) | (u16::from(high) << 8);
                    StepResult::new(20, false)
                } else {
                    StepResult::new(8, false)
                }
            }
            0xD8 => {
                // RET C - Return if carry
                if self.registers.f & FLAG_C != 0 {
                    let low = bus.read8(self.registers.sp);
                    let high = bus.read8(self.registers.sp.wrapping_add(1));
                    self.registers.sp = self.registers.sp.wrapping_add(2);
                    self.registers.pc = u16::from(low) | (u16::from(high) << 8);
                    StepResult::new(20, false)
                } else {
                    StepResult::new(8, false)
                }
            }
            0xEA => {
                let address = self.fetch16(bus);
                bus.write8(address, self.registers.a);
                StepResult::new(16, false)
            }
            0xFA => {
                let address = self.fetch16(bus);
                self.registers.a = bus.read8(address);
                StepResult::new(16, false)
            }
            0xE0 => {
                // LDH (n),A - Load A into high RAM at offset n
                let offset = self.fetch8(bus);
                bus.write8(0xFF00 + u16::from(offset), self.registers.a);
                StepResult::new(12, false)
            }
            0xF0 => {
                // LDH A,(n) - Load A from high RAM at offset n
                let offset = self.fetch8(bus);
                self.registers.a = bus.read8(0xFF00 + u16::from(offset));
                StepResult::new(12, false)
            }
            0xF3 => {
                // DI - Disable Interrupts
                self.ime = false;
                StepResult::new(4, false)
            }
            0xFB => {
                // EI - Enable Interrupts
                self.ime = true;
                StepResult::new(4, false)
            }
            0xCB => {
                let cb_opcode = self.fetch8(bus);
                self.execute_cb(cb_opcode, bus)
            }
            _ => {
                return Err(CpuError::UnimplementedOpcode {
                    opcode,
                    address: instruction_address,
                });
            }
        };

        Ok(step_result)
    }

    fn execute_cb(&mut self, opcode: u8, bus: &mut Bus) -> StepResult {
        match opcode {
            0x00..=0x07 => {
                // RLC r - Rotate left with carry
                let reg = opcode & 0x07;
                let value = self.read_reg8(reg, bus);
                let carry = value & 0x80 != 0;
                let rotated = (value << 1) | if carry { 1 } else { 0 };
                self.write_reg8(reg, rotated, bus);
                self.update_flags(rotated == 0, false, false, carry);
                StepResult::new(if reg == 6 { 16 } else { 8 }, false)
            }
            0x08..=0x0F => {
                // RRC r - Rotate right with carry
                let reg = opcode & 0x07;
                let value = self.read_reg8(reg, bus);
                let carry = value & 0x01 != 0;
                let rotated = (value >> 1) | if carry { 0x80 } else { 0 };
                self.write_reg8(reg, rotated, bus);
                self.update_flags(rotated == 0, false, false, carry);
                StepResult::new(if reg == 6 { 16 } else { 8 }, false)
            }
            0x10..=0x17 => {
                // RL r - Rotate left through carry
                let reg = opcode & 0x07;
                let value = self.read_reg8(reg, bus);
                let carry_in = if self.registers.f & FLAG_C != 0 { 1 } else { 0 };
                let carry_out = value & 0x80 != 0;
                let rotated = (value << 1) | carry_in;
                self.write_reg8(reg, rotated, bus);
                self.update_flags(rotated == 0, false, false, carry_out);
                StepResult::new(if reg == 6 { 16 } else { 8 }, false)
            }
            0x18..=0x1F => {
                // RR r - Rotate right through carry
                let reg = opcode & 0x07;
                let value = self.read_reg8(reg, bus);
                let carry_in = if self.registers.f & FLAG_C != 0 { 0x80 } else { 0 };
                let carry_out = value & 0x01 != 0;
                let rotated = (value >> 1) | carry_in;
                self.write_reg8(reg, rotated, bus);
                self.update_flags(rotated == 0, false, false, carry_out);
                StepResult::new(if reg == 6 { 16 } else { 8 }, false)
            }
            0x20..=0x27 => {
                // SLA r - Shift left arithmetic
                let reg = opcode & 0x07;
                let value = self.read_reg8(reg, bus);
                let carry = value & 0x80 != 0;
                let shifted = value << 1;
                self.write_reg8(reg, shifted, bus);
                self.update_flags(shifted == 0, false, false, carry);
                StepResult::new(if reg == 6 { 16 } else { 8 }, false)
            }
            0x28..=0x2F => {
                // SRA r - Shift right arithmetic
                let reg = opcode & 0x07;
                let value = self.read_reg8(reg, bus);
                let carry = value & 0x01 != 0;
                let msb = value & 0x80;
                let shifted = (value >> 1) | msb;
                self.write_reg8(reg, shifted, bus);
                self.update_flags(shifted == 0, false, false, carry);
                StepResult::new(if reg == 6 { 16 } else { 8 }, false)
            }
            0x30..=0x37 => {
                // SWAP r - Swap nibbles
                let reg = opcode & 0x07;
                let value = self.read_reg8(reg, bus);
                let swapped = (value >> 4) | (value << 4);
                self.write_reg8(reg, swapped, bus);
                self.update_flags(swapped == 0, false, false, false);
                StepResult::new(if reg == 6 { 16 } else { 8 }, false)
            }
            0x38..=0x3F => {
                // SRL r - Shift right logical
                let reg = opcode & 0x07;
                let value = self.read_reg8(reg, bus);
                let carry = value & 0x01 != 0;
                let shifted = value >> 1;
                self.write_reg8(reg, shifted, bus);
                self.update_flags(shifted == 0, false, false, carry);
                StepResult::new(if reg == 6 { 16 } else { 8 }, false)
            }
            0x40..=0x7F => {
                // BIT b,r - Test bit
                let bit = (opcode >> 3) & 0x07;
                let reg = opcode & 0x07;
                let value = self.read_reg8(reg, bus);
                let bit_set = value & (1 << bit) != 0;
                self.set_flag(FLAG_Z, !bit_set);
                self.set_flag(FLAG_N, false);
                self.set_flag(FLAG_H, true);
                StepResult::new(if reg == 6 { 12 } else { 8 }, false)
            }
            0x80..=0xBF => {
                // RES b,r - Reset bit
                let bit = (opcode >> 3) & 0x07;
                let reg = opcode & 0x07;
                let value = self.read_reg8(reg, bus);
                let cleared = value & !(1 << bit);
                self.write_reg8(reg, cleared, bus);
                StepResult::new(if reg == 6 { 16 } else { 8 }, false)
            }
            0xC0..=0xFF => {
                // SET b,r - Set bit
                let bit = (opcode >> 3) & 0x07;
                let reg = opcode & 0x07;
                let value = self.read_reg8(reg, bus);
                let set = value | (1 << bit);
                self.write_reg8(reg, set, bus);
                StepResult::new(if reg == 6 { 16 } else { 8 }, false)
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CpuError {
    UnimplementedOpcode { opcode: u8, address: u16 },
}

impl Display for CpuError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnimplementedOpcode { opcode, address } => {
                write!(
                    f,
                    "unimplemented opcode 0x{opcode:02X} at address 0x{address:04X}"
                )
            }
        }
    }
}

impl Error for CpuError {}
