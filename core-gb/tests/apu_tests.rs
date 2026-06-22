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
fn test_apu_register_writes_ch1() {
    let mut gb = GameBoy::from_rom_bytes(make_rom()).unwrap();
    // Turn on APU (NR52 bit 7)
    gb.bus.write8(0xFF26, 0x80);

    // Channel 1 Sweep (NR10)
    gb.bus.write8(0xFF10, 0x1E);
    assert_eq!(gb.bus.read8(0xFF10), 0x1E | 0x80); // Unused bit 7 reads as 1

    // Channel 1 Length/Duty (NR11)
    gb.bus.write8(0xFF11, 0xBF);
    assert_eq!(gb.bus.read8(0xFF11), 0xBF & 0xC0 | 0x3F); // Only duty is readable

    // Channel 1 Envelope (NR12)
    gb.bus.write8(0xFF12, 0xF3);
    assert_eq!(gb.bus.read8(0xFF12), 0xF3);

    // Channel 1 Frequency LSB (NR13)
    gb.bus.write8(0xFF13, 0xFF);
    assert_eq!(gb.bus.read8(0xFF13), 0xFF); // Write-only, usually returns FF but we might emulate differently depending on gb.

    // Channel 1 Control (NR14)
    gb.bus.write8(0xFF14, 0x86); // Trigger
    assert_eq!(gb.bus.read8(0xFF14), 0x86 & 0x40 | 0xBF); // Only Length Enable (bit 6) readable
}

#[test]
fn test_apu_register_writes_ch2() {
    let mut gb = GameBoy::from_rom_bytes(make_rom()).unwrap();
    gb.bus.write8(0xFF26, 0x80);

    // Channel 2 doesn't have a sweep register (NR20) -> FF15 unused
    gb.bus.write8(0xFF16, 0x3F); // NR21
    gb.bus.write8(0xFF17, 0xF3); // NR22
    gb.bus.write8(0xFF18, 0xFF); // NR23
    gb.bus.write8(0xFF19, 0x86); // NR24 (Trigger)
    
    assert_eq!(gb.bus.read8(0xFF17), 0xF3);
}

#[test]
fn test_apu_register_writes_ch3() {
    let mut gb = GameBoy::from_rom_bytes(make_rom()).unwrap();
    gb.bus.write8(0xFF26, 0x80);

    gb.bus.write8(0xFF1A, 0x80); // NR30 (DAC Enable)
    assert_eq!(gb.bus.read8(0xFF1A), 0x80 | 0x7F);

    gb.bus.write8(0xFF1C, 0x20); // NR32 (Volume)
    assert_eq!(gb.bus.read8(0xFF1C), 0x20 | 0x9F);

    // Wave RAM (0xFF30-0xFF3F)
    for i in 0..16 {
        gb.bus.write8(0xFF30 + i, i as u8);
        assert_eq!(gb.bus.read8(0xFF30 + i), i as u8);
    }
}

#[test]
fn test_apu_register_writes_ch4() {
    let mut gb = GameBoy::from_rom_bytes(make_rom()).unwrap();
    gb.bus.write8(0xFF26, 0x80);

    gb.bus.write8(0xFF20, 0x3F); // NR41 (Length)
    gb.bus.write8(0xFF21, 0xF3); // NR42 (Envelope)
    gb.bus.write8(0xFF22, 0x55); // NR43 (Polynomial)
    gb.bus.write8(0xFF23, 0x80); // NR44 (Trigger)

    assert_eq!(gb.bus.read8(0xFF21), 0xF3);
    assert_eq!(gb.bus.read8(0xFF22), 0x55);
}

#[test]
fn test_apu_step() {
    let mut gb = GameBoy::from_rom_bytes(make_rom()).unwrap();
    
    // Turn on APU and set master volume
    gb.bus.write8(0xFF26, 0x80);
    gb.bus.write8(0xFF24, 0x77); // NR50
    gb.bus.write8(0xFF25, 0xFF); // NR51 (All channels panned to left and right)

    // Trigger CH1
    gb.bus.write8(0xFF12, 0xF3); // Initial Vol 15, increase by 3
    gb.bus.write8(0xFF13, 0x00);
    gb.bus.write8(0xFF14, 0x80); // Trigger
    
    // Step GameBoy for a while to allow APU sequencer to tick
    gb.run_steps(100_000).unwrap();
    
    let audio = gb.take_audio_samples();
    assert!(!audio.is_empty(), "Audio buffer should have samples");
}
