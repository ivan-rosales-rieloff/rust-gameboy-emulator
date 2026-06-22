use core_gb::GameBoy;

const ROM_SIZE: usize = 0x8000;

fn make_rom(program: &[u8]) -> Vec<u8> {
    let mut rom = vec![0; ROM_SIZE];
    rom[0x0147] = 0x00; // ROM ONLY
    
    let entry_point = 0x0100usize;
    let program_len = program.len().min(ROM_SIZE - entry_point);
    rom[entry_point..entry_point + program_len].copy_from_slice(&program[..program_len]);
    
    rom
}

#[test]
fn test_cpu_loads_16bit() {
    let rom = make_rom(&[
        0x01, 0x34, 0x12, // LD BC, $1234
        0x11, 0x78, 0x56, // LD DE, $5678
        0x21, 0xBC, 0x9A, // LD HL, $9ABC
        0x31, 0xFF, 0xFF, // LD SP, $FFFF
        0xF9,             // LD SP, HL
        0x08, 0x00, 0xC0, // LD ($C000), SP
        0x76,             // HALT
    ]);
    let mut gb = GameBoy::from_rom_bytes(rom).unwrap();
    gb.run_steps(7).unwrap();
    assert_eq!(gb.registers().b, 0x12);
    assert_eq!(gb.registers().c, 0x34);
    assert_eq!(gb.registers().d, 0x56);
    assert_eq!(gb.registers().e, 0x78);
    assert_eq!(gb.registers().h, 0x9A);
    assert_eq!(gb.registers().l, 0xBC);
    assert_eq!(gb.registers().sp, 0x9ABC);
}

#[test]
fn test_cpu_stack_ops() {
    let rom = make_rom(&[
        0x31, 0x00, 0xD0, // LD SP, $D000 (WRAM)
        0x01, 0x34, 0x12, // LD BC, $1234
        0xC5,             // PUSH BC
        0x11, 0x00, 0x00, // LD DE, 0
        0xD1,             // POP DE
        0x76,             // HALT
    ]);
    let mut gb = GameBoy::from_rom_bytes(rom).unwrap();
    gb.run_steps(6).unwrap();
    assert_eq!(gb.registers().d, 0x12);
    assert_eq!(gb.registers().e, 0x34);
}

#[test]
fn test_cpu_16bit_alu() {
    let rom = make_rom(&[
        0x21, 0x00, 0x10, // LD HL, $1000
        0x01, 0x00, 0x20, // LD BC, $2000
        0x09,             // ADD HL, BC -> HL = $3000
        0x23,             // INC HL -> HL = $3001
        0x0B,             // DEC BC -> BC = $1FFF
        0x76,             // HALT
    ]);
    let mut gb = GameBoy::from_rom_bytes(rom).unwrap();
    gb.run_steps(6).unwrap();
    assert_eq!(gb.registers().h, 0x30);
    assert_eq!(gb.registers().l, 0x01);
    assert_eq!(gb.registers().b, 0x1F);
    assert_eq!(gb.registers().c, 0xFF);
}

#[test]
fn test_cpu_misc() {
    let rom = make_rom(&[
        0x3F, // CCF
        0x37, // SCF
        0x00, // NOP
        0x27, // DAA
        0x2F, // CPL
        0x76, // HALT
    ]);
    let mut gb = GameBoy::from_rom_bytes(rom).unwrap();
    gb.run_steps(6).unwrap();
}

#[test]
fn test_cpu_rst() {
    let rom = make_rom(&[
        0x31, 0x00, 0xD0, // LD SP, $D000
        0xDF,             // RST 18H -> PC=0x0018
    ]);
    let mut gb = GameBoy::from_rom_bytes(rom).unwrap();
    gb.run_steps(2).unwrap();
    assert_eq!(gb.pc(), 0x0018);
}
