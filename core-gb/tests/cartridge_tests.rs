use core_gb::GameBoy;

const ROM_SIZE: usize = 0x8000;

fn make_rom(cartridge_type: u8, size: usize) -> Vec<u8> {
    let mut rom = vec![0; size];
    rom[0x0147] = cartridge_type; // Cartridge type
    rom[0x0148] = if size > 0x8000 { 1 } else { 0 }; // ROM size
    rom[0x0149] = 1; // RAM size
    rom
}

#[test]
fn test_mbc1_cartridge() {
    // MBC1 (0x01)
    let mut rom = make_rom(0x01, 0x40000); // 256KB ROM
    rom[0x4000] = 0xAA; // Bank 1
    
    let mut gb = GameBoy::from_rom_bytes(rom).unwrap();
    
    // Switch to ROM Bank 1
    gb.bus.write8(0x2100, 0x01); 
    assert_eq!(gb.bus.read8(0x4000), 0xAA);
}

#[test]
fn test_mbc3_rtc_unimplemented() {
    // MBC3+TIMER+BATTERY (0x0F)
    let rom = make_rom(0x0F, 0x10000);
    let mut gb = GameBoy::from_rom_bytes(rom).unwrap();
    
    // Enable RAM/RTC
    gb.bus.write8(0x0000, 0x0A);
    
    // Select RTC Register 0x08 (Seconds)
    gb.bus.write8(0x4000, 0x08);
    
    // Write/Read RTC Register (Unimplemented, returns 0)
    gb.bus.write8(0xA000, 0x30);
    assert_eq!(gb.bus.read8(0xA000), 0x00);
}
