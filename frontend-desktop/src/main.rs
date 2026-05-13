use std::env;
use std::fs;
use std::process;

use core_gb::GameBoy;
use minifb::{Key, Scale, Window, WindowOptions};

const PALETTE: [u32; 4] = [0xFFFFFFFF, 0xAAAAAAFF, 0x555555FF, 0x000000FF];

// Joypad button bits (active high in our representation)
const BTN_A: u8 = 0x01;
const BTN_B: u8 = 0x02;
const BTN_SELECT: u8 = 0x04;
const BTN_START: u8 = 0x08;
const BTN_RIGHT: u8 = 0x10;
const BTN_LEFT: u8 = 0x20;
const BTN_UP: u8 = 0x40;
const BTN_DOWN: u8 = 0x80;

fn main() {
    let rom_path = parse_args();

    let rom_bytes = fs::read(&rom_path).unwrap_or_else(|error| {
        eprintln!("Failed to read ROM at '{rom_path}': {error}");
        process::exit(1);
    });

    let mut game_boy = GameBoy::from_rom_bytes(rom_bytes).unwrap_or_else(|error| {
        eprintln!("Failed to initialize Game Boy core: {error}");
        process::exit(1);
    });

    let (width, height) = GameBoy::screen_dimensions();
    let mut window = Window::new(
        &format!("Game Boy Emulator - {}", game_boy.title()),
        width,
        height,
        WindowOptions {
            scale: Scale::X4,
            ..WindowOptions::default()
        },
    )
    .unwrap_or_else(|error| {
        eprintln!("Failed to create window: {error}");
        process::exit(1);
    });

    let mut buffer = vec![0u32; width * height];

    while window.is_open() && !window.is_key_down(Key::Escape) {
        // Read keyboard input and update joypad state
        let buttons = read_buttons(&window);
        game_boy.set_button_state(buttons);

        if let Err(error) = game_boy.run_frame() {
            eprintln!("Emulation error: {error}");
            process::exit(1);
        }

        for (pixel_index, &pixel_value) in game_boy.framebuffer().iter().enumerate() {
            buffer[pixel_index] = PALETTE[pixel_value as usize];
        }

        window.update_with_buffer(&buffer, width, height).unwrap();
    }
}

fn read_buttons(window: &Window) -> u8 {
    let mut buttons = 0u8;

    if window.is_key_pressed(Key::Z, minifb::KeyRepeat::Yes) {
        buttons |= BTN_A;
    }
    if window.is_key_pressed(Key::X, minifb::KeyRepeat::Yes) {
        buttons |= BTN_B;
    }
    if window.is_key_pressed(Key::Space, minifb::KeyRepeat::Yes) {
        buttons |= BTN_SELECT;
    }
    if window.is_key_pressed(Key::Enter, minifb::KeyRepeat::Yes) {
        buttons |= BTN_START;
    }
    if window.is_key_pressed(Key::Right, minifb::KeyRepeat::Yes) {
        buttons |= BTN_RIGHT;
    }
    if window.is_key_pressed(Key::Left, minifb::KeyRepeat::Yes) {
        buttons |= BTN_LEFT;
    }
    if window.is_key_pressed(Key::Up, minifb::KeyRepeat::Yes) {
        buttons |= BTN_UP;
    }
    if window.is_key_pressed(Key::Down, minifb::KeyRepeat::Yes) {
        buttons |= BTN_DOWN;
    }

    buttons
}

fn parse_args() -> String {
    let mut args = env::args().skip(1);
    match args.next() {
        Some(path) => path,
        None => {
            print_usage_and_exit();
            unreachable!();
        }
    }
}

fn print_usage_and_exit() {
    eprintln!("Usage: frontend-desktop <path-to-rom.gb>");
    process::exit(2);
}
