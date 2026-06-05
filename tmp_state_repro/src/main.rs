use bincode;
use core_gb::{Cartridge, GameBoy};

fn main() {
    println!("Testing Cartridge decode...");
    let rom = vec![0u8; 0x8000];
    let cartridge = Cartridge::from_rom(rom).expect("create cartridge");
    let encoded_cart = bincode::serde::encode_to_vec(&cartridge, bincode::config::standard()).expect("encode cartridge");
    let (_decoded_cart, _) = bincode::serde::decode_from_slice::<Cartridge, _>(&encoded_cart, bincode::config::standard()).expect("decode cartridge");
    println!("Cartridge decode OK");

    println!("Testing GameBoy save/load state...");
    let mut rom = vec![0u8; 0x8000];
    rom[0x0147] = 0x00;
    rom[0x0100] = 0x00;
    let gb = GameBoy::from_rom_bytes(rom).expect("create gameboy");
    let path = std::env::temp_dir().join("gameboy_state_repro.state");
    gb.save_state(&path).expect("save state");
    let _loaded_gb = GameBoy::load_state(&path).expect("load state");
    println!("GameBoy save/load state OK");
}

