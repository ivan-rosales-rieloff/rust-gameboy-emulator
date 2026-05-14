use std::fs;
use std::path::PathBuf;

use core_gb::GameBoy;

const ROMS: &[&str] = &["../PokemonRed.gb", "../Pokemon - Red Version (USA, Europe) (SGB Enhanced).gb", "../dmg-acid2.gb"];

fn load_rom(path: &str) -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(path);
    fs::read(&path).unwrap_or_else(|error| panic!("Failed to read ROM '{}': {}", path.display(), error))
}

fn framebuffer_is_not_blank(framebuffer: &[u8]) -> bool {
    framebuffer.iter().any(|&pixel| pixel != framebuffer[0])
}

#[test]
fn all_roms_render_non_blank_frame() {
    for &rom_path in ROMS {
        let rom = load_rom(rom_path);
        let mut game_boy = GameBoy::from_rom_bytes(rom).unwrap();

        // Run several frames to allow the game to initialize and draw.
        let mut found_non_blank = false;
        for _ in 0..150 {
            game_boy.run_frame().unwrap();
            let framebuffer = game_boy.framebuffer();
            if framebuffer_is_not_blank(framebuffer) {
                found_non_blank = true;
                break;
            }
        }

        assert!(found_non_blank, "ROM '{}' produced a blank framebuffer", rom_path);
    }
}
