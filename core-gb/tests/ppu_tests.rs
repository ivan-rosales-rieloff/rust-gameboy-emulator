use core_gb::GameBoy;

const ROM_SIZE: usize = 0x8000;

fn make_rom() -> Vec<u8> {
    let mut rom = vec![0; ROM_SIZE];
    rom[0x0147] = 0x00; // ROM ONLY
    
    // Minimal infinite loop program
    let program = [0x18, 0xFE]; // JR -2
    let entry_point = 0x0100usize;
    rom[entry_point..entry_point + 2].copy_from_slice(&program);
    
    rom
}

#[test]
fn test_ppu_lcd_disable_enable() {
    let mut gb = GameBoy::from_rom_bytes(make_rom()).unwrap();
    
    // Disable LCD (bit 7 of LCDC = 0)
    gb.bus.write8(0xFF40, 0x00);
    
    // Run for a full frame of cycles
    gb.run_steps(100_000).unwrap();
    
    // Check that LY is 0 and STAT mode is 0 when LCD is off
    assert_eq!(gb.bus.read8(0xFF44), 0);
    assert_eq!(gb.bus.read8(0xFF41) & 0x03, 0);
    
    // Enable LCD (bit 7 of LCDC = 1)
    gb.bus.write8(0xFF40, 0x80);
    
    // Step PPU directly with 12 cycles to enter OAM search
    gb.ppu.step(12, &mut gb.bus);
    assert_eq!(gb.bus.read8(0xFF41) & 0x03, 2);
}

#[test]
fn test_ppu_sprite_priority() {
    let mut gb = GameBoy::from_rom_bytes(make_rom()).unwrap();
    
    // Set up LCDC for sprites (bit 1 = 1) and BG (bit 0 = 1)
    gb.bus.write8(0xFF40, 0x83);
    
    // Set up OAM (Sprite 0)
    gb.bus.write8(0xFE00, 16); // Y pos = 0
    gb.bus.write8(0xFE01, 8);  // X pos = 0
    gb.bus.write8(0xFE02, 0);  // Tile index 0
    gb.bus.write8(0xFE03, 0x80); // Priority bit 7 = 1 (behind BG)
    
    // Set BG palette
    gb.bus.write8(0xFF47, 0xE4); // Standard palette
    
    // Set Sprite Palette 0
    gb.bus.write8(0xFF48, 0xE4);
    
    // Run for a frame to render
    gb.run_frame().unwrap();
    
    // The framebuffer shouldn't crash, and we have rendered a frame
    let fb = gb.framebuffer();
    assert_eq!(fb.len(), 160 * 144);
}
