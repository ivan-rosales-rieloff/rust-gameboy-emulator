use std::error::Error;
use std::fmt::{Display, Formatter};

use core_common::StepResult;

use crate::bus::Bus;
use crate::trace::{trace, trace_enabled};

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

    pub fn ime_enabled(&self) -> bool {
        self.ime
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

    fn add_hl(&mut self, value: u16) {
        let hl = self.hl();
        let result = hl.wrapping_add(value);
        let half = (hl & 0x0FFF) + (value & 0x0FFF) > 0x0FFF;
        let carry = u32::from(hl) + u32::from(value) > 0xFFFF;
        self.registers.f &= !(FLAG_N | FLAG_H | FLAG_C);
        self.set_flag(FLAG_H, half);
        self.set_flag(FLAG_C, carry);
        self.set_hl(result);
    }

    fn add_sp_signed(&mut self, offset: i8) {
        let sp = self.registers.sp;
        let value = offset as u16;
        let result = sp.wrapping_add(value);
        let half = (sp & 0x0F) + (value & 0x0F) > 0x0F;
        let carry = (sp & 0xFF) + (value & 0xFF) > 0xFF;

        self.registers.f = 0;
        self.set_flag(FLAG_H, half);
        self.set_flag(FLAG_C, carry);
        self.registers.sp = result;
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

    fn adc_a(&mut self, value: u8) {
        let a = self.registers.a;
        let carry = (self.registers.f & FLAG_C) != 0;
        let carry_val = if carry { 1 } else { 0 };
        let (intermediate, carry1) = a.overflowing_add(value);
        let (result, carry2) = intermediate.overflowing_add(carry_val);
        let half = (a & 0x0F) + (value & 0x0F) + carry_val > 0x0F;
        self.registers.a = result;
        self.update_flags(result == 0, false, half, carry1 || carry2);
    }

    fn sbc_a(&mut self, value: u8) {
        let a = self.registers.a;
        let carry = (self.registers.f & FLAG_C) != 0;
        let carry_val = if carry { 1 } else { 0 };
        let (intermediate, borrow1) = a.overflowing_sub(value);
        let (result, borrow2) = intermediate.overflowing_sub(carry_val);
        let half = (a & 0x0F) < ((value & 0x0F) + carry_val);
        self.registers.a = result;
        self.update_flags(result == 0, true, half, borrow1 || borrow2);
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

    fn push16(&mut self, value: u16, bus: &mut Bus) {
        self.registers.sp = self.registers.sp.wrapping_sub(2);
        bus.write8(self.registers.sp, value as u8);
        bus.write8(self.registers.sp.wrapping_add(1), (value >> 8) as u8);
    }

    fn pop16(&mut self, bus: &mut Bus) -> u16 {
        let low = bus.read8(self.registers.sp);
        let high = bus.read8(self.registers.sp.wrapping_add(1));
        self.registers.sp = self.registers.sp.wrapping_add(2);
        u16::from(low) | (u16::from(high) << 8)
    }

    fn pending_interrupts(&self, bus: &Bus) -> u8 {
        let interrupt_enable = bus.read8(0xFFFF);
        let interrupt_flags = bus.read8(0xFF0F);
        interrupt_enable & interrupt_flags
    }

    fn service_interrupt(&mut self, bus: &mut Bus) -> Option<StepResult> {
        let active = self.pending_interrupts(bus);
        if active == 0 || !self.ime {
            return None;
        }

        let (flag, vector) = if active & 0x01 != 0 {
            (0x01, 0x40)
        } else if active & 0x02 != 0 {
            (0x02, 0x48)
        } else if active & 0x04 != 0 {
            (0x04, 0x50)
        } else if active & 0x08 != 0 {
            (0x08, 0x58)
        } else if active & 0x10 != 0 {
            (0x10, 0x60)
        } else {
            return None;
        };

        self.halted = false;
        self.ime = false;
        self.push16(self.registers.pc, bus);
        let current_if = bus.read8(0xFF0F);
        bus.write8(0xFF0F, current_if & !flag);
        self.registers.pc = vector;
        Some(StepResult::new(20, false))
    }

    pub fn step(&mut self, bus: &mut Bus) -> Result<StepResult, CpuError> {
        if let Some(interrupt_step) = self.service_interrupt(bus) {
            return Ok(interrupt_step);
        }

        if self.halted {
            if self.pending_interrupts(bus) != 0 {
                self.halted = false;
            } else {
                return Ok(StepResult::new(4, true));
            }
        }

        let instruction_address = self.registers.pc;
        let opcode = self.fetch8(bus);

        if trace_enabled() {
            // Log key Pokemon Red code regions and joypad interaction candidates.
            if instruction_address == 0x614F
                || instruction_address == 0x28CE
                || instruction_address == 0x28CB
                || instruction_address == 0x2061
                || instruction_address == 0x1F84
                || instruction_address == 0x1F8E
            {
                trace(&format!(
                    "CPU trace: PC=0x{instruction_address:04X} opcode=0x{opcode:02X} A=0x{a:02X} F=0x{f:02X} B=0x{b:02X} C=0x{c:02X} D=0x{d:02X} E=0x{e:02X} H=0x{h:02X} L=0x{l:02X} SP=0x{sp:04X}",
                    a = self.registers.a,
                    f = self.registers.f,
                    b = self.registers.b,
                    c = self.registers.c,
                    d = self.registers.d,
                    e = self.registers.e,
                    h = self.registers.h,
                    l = self.registers.l,
                    sp = self.registers.sp,
                ));
            }
        }

        let step_result = match opcode {
            0x00 => StepResult::new(4, false),
            0x04..=0x3C if opcode & 0x07 == 0x04 => {
                let reg = (opcode >> 3) & 0x07;
                let value = self.read_reg8(reg, bus);
                let updated = self.inc8(value);
                self.write_reg8(reg, updated, bus);
                StepResult::new(if reg == 6 { 12 } else { 4 }, false)
            }
            0x05..=0x3D if opcode & 0x07 == 0x05 => {
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
            0x27 => {
                // DAA
                let a = self.registers.a;
                let mut adjust = 0;
                let mut carry = self.registers.f & FLAG_C != 0;
                if self.registers.f & FLAG_H != 0 || (a & 0x0F) > 9 {
                    adjust |= 0x06;
                }
                if carry || a > 0x99 {
                    adjust |= 0x60;
                    carry = true;
                }
                let result = if self.registers.f & FLAG_N != 0 {
                    a.wrapping_sub(adjust)
                } else {
                    a.wrapping_add(adjust)
                };
                self.registers.a = result;
                self.set_flag(FLAG_Z, result == 0);
                self.set_flag(FLAG_H, false);
                self.set_flag(FLAG_C, carry);
                StepResult::new(4, false)
            }
            0x2F => {
                // CPL
                self.registers.a = !self.registers.a;
                self.set_flag(FLAG_N, true);
                self.set_flag(FLAG_H, true);
                StepResult::new(4, false)
            }
            0x3C => {
                // INC A
                self.registers.a = self.inc8(self.registers.a);
                StepResult::new(4, false)
            }
            0x07 => {
                // RLCA
                let carry = self.registers.a & 0x80 != 0;
                self.registers.a = self.registers.a.rotate_left(1);
                self.set_flag(FLAG_Z, false);
                self.set_flag(FLAG_N, false);
                self.set_flag(FLAG_H, false);
                self.set_flag(FLAG_C, carry);
                StepResult::new(4, false)
            }
            0x0F => {
                // RRCA
                let carry = self.registers.a & 0x01 != 0;
                self.registers.a = self.registers.a.rotate_right(1);
                self.set_flag(FLAG_Z, false);
                self.set_flag(FLAG_N, false);
                self.set_flag(FLAG_H, false);
                self.set_flag(FLAG_C, carry);
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
            0x09 => {
                self.add_hl(self.bc());
                StepResult::new(8, false)
            }
            0x19 => {
                self.add_hl(self.de());
                StepResult::new(8, false)
            }
            0x29 => {
                self.add_hl(self.hl());
                StepResult::new(8, false)
            }
            0x39 => {
                self.add_hl(self.registers.sp);
                StepResult::new(8, false)
            }
            0x31 => {
                let value = self.fetch16(bus);
                self.set_sp(value);
                StepResult::new(12, false)
            }
            0xE8 => {
                let offset = self.fetch8(bus) as i8;
                self.add_sp_signed(offset);
                StepResult::new(16, false)
            }
            0xF8 => {
                let offset = self.fetch8(bus) as i8;
                let sp = self.registers.sp;
                let value = offset as u16;
                let result = sp.wrapping_add(value);
                let half = (sp & 0x0F) + (value & 0x0F) > 0x0F;
                let carry = (sp & 0xFF) + (value & 0xFF) > 0xFF;
                self.registers.f = 0;
                self.set_flag(FLAG_H, half);
                self.set_flag(FLAG_C, carry);
                self.set_hl(result);
                StepResult::new(12, false)
            }
            0xF9 => {
                self.registers.sp = self.hl();
                StepResult::new(8, false)
            }
            0xE9 => {
                self.registers.pc = self.hl();
                StepResult::new(4, false)
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
            0x88..=0x8F => {
                let value = self.read_reg8(opcode & 0x07, bus);
                self.adc_a(value);
                StepResult::new(4, false)
            }
            0x90..=0x97 => {
                let value = self.read_reg8(opcode & 0x07, bus);
                self.sub_a(value);
                StepResult::new(4, false)
            }
            0x98..=0x9F => {
                let value = self.read_reg8(opcode & 0x07, bus);
                self.sbc_a(value);
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
            0xC5 => {
                self.push16(self.bc(), bus);
                StepResult::new(16, false)
            }
            0xD5 => {
                self.push16(self.de(), bus);
                StepResult::new(16, false)
            }
            0xE5 => {
                self.push16(self.hl(), bus);
                StepResult::new(16, false)
            }
            0xF5 => {
                let value = u16::from(self.registers.a) << 8 | u16::from(self.registers.f & 0xF0);
                self.push16(value, bus);
                StepResult::new(16, false)
            }
            0xC1 => {
                let value = self.pop16(bus);
                self.registers.b = (value >> 8) as u8;
                self.registers.c = value as u8;
                StepResult::new(12, false)
            }
            0xD1 => {
                let value = self.pop16(bus);
                self.registers.d = (value >> 8) as u8;
                self.registers.e = value as u8;
                StepResult::new(12, false)
            }
            0xE1 => {
                let value = self.pop16(bus);
                self.registers.h = (value >> 8) as u8;
                self.registers.l = value as u8;
                StepResult::new(12, false)
            }
            0xF1 => {
                let value = self.pop16(bus);
                self.registers.a = (value >> 8) as u8;
                self.registers.f = (value as u8) & 0xF0;
                StepResult::new(12, false)
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
            0xC4 => {
                let address = self.fetch16(bus);
                if self.registers.f & FLAG_Z == 0 {
                    let return_address = self.registers.pc;
                    self.registers.sp = self.registers.sp.wrapping_sub(2);
                    bus.write8(self.registers.sp, (return_address & 0xFF) as u8);
                    bus.write8(self.registers.sp.wrapping_add(1), (return_address >> 8) as u8);
                    self.registers.pc = address;
                    StepResult::new(24, false)
                } else {
                    StepResult::new(12, false)
                }
            }
            0xCC => {
                let address = self.fetch16(bus);
                if self.registers.f & FLAG_Z != 0 {
                    let return_address = self.registers.pc;
                    self.registers.sp = self.registers.sp.wrapping_sub(2);
                    bus.write8(self.registers.sp, (return_address & 0xFF) as u8);
                    bus.write8(self.registers.sp.wrapping_add(1), (return_address >> 8) as u8);
                    self.registers.pc = address;
                    StepResult::new(24, false)
                } else {
                    StepResult::new(12, false)
                }
            }
            0xD4 => {
                let address = self.fetch16(bus);
                if self.registers.f & FLAG_C == 0 {
                    let return_address = self.registers.pc;
                    self.registers.sp = self.registers.sp.wrapping_sub(2);
                    bus.write8(self.registers.sp, (return_address & 0xFF) as u8);
                    bus.write8(self.registers.sp.wrapping_add(1), (return_address >> 8) as u8);
                    self.registers.pc = address;
                    StepResult::new(24, false)
                } else {
                    StepResult::new(12, false)
                }
            }
            0xDC => {
                let address = self.fetch16(bus);
                if self.registers.f & FLAG_C != 0 {
                    let return_address = self.registers.pc;
                    self.registers.sp = self.registers.sp.wrapping_sub(2);
                    bus.write8(self.registers.sp, (return_address & 0xFF) as u8);
                    bus.write8(self.registers.sp.wrapping_add(1), (return_address >> 8) as u8);
                    self.registers.pc = address;
                    StepResult::new(24, false)
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
            0xD9 => {
                let low = bus.read8(self.registers.sp);
                let high = bus.read8(self.registers.sp.wrapping_add(1));
                self.registers.sp = self.registers.sp.wrapping_add(2);
                self.registers.pc = u16::from(low) | (u16::from(high) << 8);
                self.ime = true;
                StepResult::new(16, false)
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
            0xE2 => {
                // LD (C),A - Load A into high RAM at offset C
                let address = 0xFF00 + u16::from(self.registers.c);
                bus.write8(address, self.registers.a);
                StepResult::new(8, false)
            }
            0xE6 => {
                let value = self.fetch8(bus);
                self.and_a(value);
                StepResult::new(8, false)
            }
            0xEE => {
                let value = self.fetch8(bus);
                self.xor_a(value);
                StepResult::new(8, false)
            }
            0xF0 => {
                // LDH A,(n) - Load A from high RAM at offset n
                let offset = self.fetch8(bus);
                self.registers.a = bus.read8(0xFF00 + u16::from(offset));
                StepResult::new(12, false)
            }
            0xF2 => {
                // LD A,(C) - Load A from high RAM at offset C
                self.registers.a = bus.read8(0xFF00 + u16::from(self.registers.c));
                StepResult::new(8, false)
            }
            0xF3 => {
                // DI - Disable Interrupts
                self.ime = false;
                StepResult::new(4, false)
            }
            0x36 => {
                let value = self.fetch8(bus);
                bus.write8(self.hl(), value);
                StepResult::new(12, false)
            }
            0xC6 => {
                let value = self.fetch8(bus);
                self.add_a(value);
                StepResult::new(8, false)
            }
            0xCE => {
                let value = self.fetch8(bus);
                let carry = self.registers.f & FLAG_C != 0;
                let carry_val = if carry { 1 } else { 0 };
                let a = self.registers.a;
                let (intermediate, carry1) = a.overflowing_add(value);
                let (result, carry2) = intermediate.overflowing_add(carry_val);
                let half = (a & 0x0F) + (value & 0x0F) + carry_val > 0x0F;
                self.registers.a = result;
                self.update_flags(result == 0, false, half, carry1 || carry2);
                StepResult::new(8, false)
            }
            0xD6 => {
                let value = self.fetch8(bus);
                self.sub_a(value);
                StepResult::new(8, false)
            }
            0xDE => {
                let value = self.fetch8(bus);
                self.sbc_a(value);
                StepResult::new(8, false)
            }
            0xF6 => {
                let value = self.fetch8(bus);
                self.or_a(value);
                StepResult::new(8, false)
            }
            0xFE => {
                let value = self.fetch8(bus);
                self.cp_a(value);
                StepResult::new(8, false)
            }
            0x37 => {
                // SCF
                self.set_flag(FLAG_N, false);
                self.set_flag(FLAG_H, false);
                self.set_flag(FLAG_C, true);
                StepResult::new(4, false)
            }
            0x3F => {
                // CCF
                self.set_flag(FLAG_N, false);
                self.set_flag(FLAG_H, false);
                self.set_flag(FLAG_C, self.registers.f & FLAG_C == 0);
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
