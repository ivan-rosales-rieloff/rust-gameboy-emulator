use bincode;
use core_gb::{Cartridge, GameBoy};

fn main() {
    println!("Testing Cartridge decode...");
    let rom = vec![0u8; 0x8000];
    let cartridge = Cartridge::from_rom(rom).expect("create cartridge");
    let encoded_cart = bincode::serde::encode_to_vec(&cartridge, bincode::config::standard()).expect("encode cartridge");
    let (_decoded_cart, _) = bincode::serde::decode_from_slice::<Cartridge, _>(&encoded_cart, bincode::config::standard()).expect("decode cartridge");
    println!("Cartridge decode OK");

    println!("Testing GameBoy decode...");
    let mut rom = vec![0u8; 0x8000];
    rom[0x0147] = 0x00;
    rom[0x0100] = 0x00;
    let mut gb = GameBoy::from_rom_bytes(rom).expect("create gameboy");
    gb.run_steps(1000).expect("run steps");
    let encoded_gb = bincode::serde::encode_to_vec(&gb, bincode::config::standard()).expect("encode gb");
    let (_decoded_gb, _) = bincode::serde::decode_from_slice::<GameBoy, _>(&encoded_gb, bincode::config::standard()).expect("decode gb");
    println!("GameBoy decode OK");
}

