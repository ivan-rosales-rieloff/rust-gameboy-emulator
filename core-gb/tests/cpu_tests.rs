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
fn test_alu_add_sub() {
    let rom = make_rom(&[
        0x3E, 0x50, // LD A, $50
        0x06, 0x10, // LD B, $10
        0x80,       // ADD A, B -> A=$60
        0x2E, 0x20, // LD L, $20
        0x95,       // SUB L -> A=$40
        0x76,       // HALT
    ]);
    let mut gb = GameBoy::from_rom_bytes(rom).unwrap();
    gb.run_steps(6).unwrap();
    assert_eq!(gb.registers().a, 0x40);
    assert_eq!(gb.registers().f & 0x40, 0x40); // N flag set after SUB
}

#[test]
fn test_alu_and_or_xor() {
    let rom = make_rom(&[
        0x3E, 0xFF, // LD A, $FF
        0xE6, 0x0F, // AND $0F -> A=$0F
        0xF6, 0x10, // OR $10 -> A=$1F
        0xEE, 0x11, // XOR $11 -> A=$0E
        0x76,       // HALT
    ]);
    let mut gb = GameBoy::from_rom_bytes(rom).unwrap();
    gb.run_steps(5).unwrap();
    assert_eq!(gb.registers().a, 0x0E);
}

#[test]
fn test_control_flow_jp_jr() {
    let program = vec![
        0x3E, 0x01, // LD A, $01
        0xC3, 0x07, 0x01, // JP 0x0107
        0x3E, 0x02, // LD A, $02 (skipped)
        0x18, 0x02, // JR +2 (to 0x010B)
        0x3E, 0x03, // LD A, $03 (skipped)
        0x76,       // HALT (at 0x010B)
    ];
    let rom = make_rom(&program);
    let mut gb = GameBoy::from_rom_bytes(rom).unwrap();
    gb.run_steps(4).unwrap();
    assert_eq!(gb.registers().a, 0x01);
}

#[test]
fn test_control_flow_call_ret() {
    let rom = make_rom(&[
        0x31, 0xFE, 0xFF, // LD SP, $FFFE
        0xCD, 0x09, 0x01, // CALL 0x0109
        0x76,             // HALT (0x0106, actually stops at 0x0106+1)
        0x00, 0x00,
        0x3E, 0x99,       // LD A, $99 (0x0109)
        0xC9,             // RET
    ]);
    let mut gb = GameBoy::from_rom_bytes(rom).unwrap();
    gb.run_steps(5).unwrap(); // LD SP, CALL, LD A, RET, HALT
    assert_eq!(gb.registers().a, 0x99);
}

#[test]
fn test_cb_bit_res_set() {
    let rom = make_rom(&[
        0x06, 0x80, // LD B, $80 (bit 7 set)
        0xCB, 0x78, // BIT 7, B -> Z=0
        0xCB, 0x80, // RES 0, B -> B=$80
        0xCB, 0xC0, // SET 0, B -> B=$81
        0x76,       // HALT
    ]);
    let mut gb = GameBoy::from_rom_bytes(rom).unwrap();
    gb.run_steps(5).unwrap();
    assert_eq!(gb.registers().b, 0x81);
    assert_eq!(gb.registers().f & 0x80, 0x00); // Z=0
}

#[test]
fn test_cb_shifts_rotates() {
    let rom = make_rom(&[
        0x3E, 0x01, // LD A, $01
        0xCB, 0x07, // RLC A -> A=$02
        0xCB, 0x0F, // RRC A -> A=$01
        0xCB, 0x27, // SLA A -> A=$02
        0xCB, 0x3F, // SRL A -> A=$01
        0x76,       // HALT
    ]);
    let mut gb = GameBoy::from_rom_bytes(rom).unwrap();
    gb.run_steps(6).unwrap();
    assert_eq!(gb.registers().a, 0x01);
}
