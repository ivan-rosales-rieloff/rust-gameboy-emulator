use core_gb::GameBoy;

const ROM_SIZE: usize = 0x8000;

fn make_rom() -> Vec<u8> {
    let mut rom = vec![0; ROM_SIZE];
    rom[0x0147] = 0x00; // ROM ONLY
    rom
}

#[test]
fn test_bus_vram_read_write() {
    let mut gb = GameBoy::from_rom_bytes(make_rom()).unwrap();
    
    // Write to VRAM Bank 0
    gb.bus.write8(0x8000, 0xAB);
    assert_eq!(gb.bus.read8(0x8000), 0xAB);

    // Write to VRAM Bank 1 (requires CGB mode, so we check if writes pass or fail based on mode)
    if gb.bus.is_cgb {
        gb.bus.write8(0xFF4F, 1); // Select VRAM Bank 1
        gb.bus.write8(0x8000, 0xCD);
        assert_eq!(gb.bus.read8(0x8000), 0xCD);
        
        gb.bus.write8(0xFF4F, 0); // Back to Bank 0
        assert_eq!(gb.bus.read8(0x8000), 0xAB);
    }
}

#[test]
fn test_bus_wram_read_write() {
    let mut gb = GameBoy::from_rom_bytes(make_rom()).unwrap();
    
    // Write to fixed WRAM Bank 0 (0xC000-0xCFFF)
    gb.bus.write8(0xC000, 0x12);
    assert_eq!(gb.bus.read8(0xC000), 0x12);
    
    // Write to switchable WRAM Bank 1 (0xD000-0xDFFF)
    gb.bus.write8(0xD000, 0x34);
    assert_eq!(gb.bus.read8(0xD000), 0x34);

    // Echo RAM (0xE000-0xFDFF) mirrors WRAM (0xC000-0xDDFF)
    assert_eq!(gb.bus.read8(0xE000), 0x12);
    
    if gb.bus.is_cgb {
        gb.bus.write8(0xFF70, 2); // Select WRAM Bank 2
        gb.bus.write8(0xD000, 0x56);
        assert_eq!(gb.bus.read8(0xD000), 0x56);
        
        gb.bus.write8(0xFF70, 1); // Back to Bank 1
        assert_eq!(gb.bus.read8(0xD000), 0x34);
    }
}

#[test]
fn test_bus_hram_and_oam() {
    let mut gb = GameBoy::from_rom_bytes(make_rom()).unwrap();
    
    // Write to OAM
    gb.bus.write8(0xFE00, 0x99);
    assert_eq!(gb.bus.read8(0xFE00), 0x99);
    
    // Write to HRAM
    gb.bus.write8(0xFF80, 0x77);
    assert_eq!(gb.bus.read8(0xFF80), 0x77);
}

#[test]
fn test_bus_io_registers() {
    let mut gb = GameBoy::from_rom_bytes(make_rom()).unwrap();
    
    // SB and SC (Serial)
    gb.bus.write8(0xFF01, 0xAA);
    assert_eq!(gb.bus.read8(0xFF01), 0xAA);
    gb.bus.write8(0xFF02, 0x81); // Start transfer
    
    // Timer registers
    gb.bus.write8(0xFF04, 0x00); // DIV (writing resets it)
    assert_eq!(gb.bus.read8(0xFF04), 0x00);
    
    gb.bus.write8(0xFF05, 0xBB); // TIMA
    assert_eq!(gb.bus.read8(0xFF05), 0xBB);
    
    gb.bus.write8(0xFF06, 0xCC); // TMA
    assert_eq!(gb.bus.read8(0xFF06), 0xCC);
    
    gb.bus.write8(0xFF07, 0x05); // TAC
    assert_eq!(gb.bus.read8(0xFF07), 0x05);
    
    // Interrupt Flag (IF) and Enable (IE)
    gb.bus.write8(0xFF0F, 0x1F);
    assert_eq!(gb.bus.read8(0xFF0F), 0x1F);
    
    gb.bus.write8(0xFFFF, 0x1F);
    assert_eq!(gb.bus.read8(0xFFFF), 0x1F);
}
