use std::env;
use std::fs;
use std::path::PathBuf;
use std::process;

use core_gb::GameBoy;
use minifb::{Key, Scale, Window, WindowOptions};
use rfd::FileDialog;

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
    let mut game_boy = env::args().nth(1).map(PathBuf::from).map(load_rom).transpose().unwrap_or_else(|error| {
        eprintln!("{error}");
        process::exit(1);
    });

    let (width, height) = GameBoy::screen_dimensions();
    let mut window = Window::new(
        "Game Boy Emulator",
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
        if window.is_key_pressed(Key::L, minifb::KeyRepeat::No) {
            if let Some(game) = &game_boy {
                save_current_game(game);
            }

            if let Some(path) = FileDialog::new()
                .add_filter("Game Boy ROM", &[
                    "gb",
                    "gbc",
                ])
                .pick_file()
            {
                match load_rom(path) {
                    Ok(loaded_game) => {
                        game_boy = Some(loaded_game);
                    }
                    Err(error) => {
                        eprintln!("Failed to load ROM: {error}");
                    }
                }
            }
        }

        if let Some(game_boy) = &mut game_boy {
            let buttons = read_buttons(&window);
            game_boy.set_button_state(buttons);

            if let Err(error) = game_boy.run_frame() {
                eprintln!("Emulation error: {error}");
                process::exit(1);
            }

            for (pixel_index, &pixel_value) in game_boy.framebuffer().iter().enumerate() {
                buffer[pixel_index] = PALETTE[pixel_value as usize];
            }
        } else {
            render_no_rom_screen(&mut buffer, width, height);
        }

        window.update_with_buffer(&buffer, width, height).unwrap();
    }

    if let Some(game_boy) = &game_boy {
        if game_boy.has_battery() {
            if let Err(error) = game_boy.save_game() {
                eprintln!("Failed to save game: {error}");
            } else {
                println!("Game saved successfully!");
            }
        }
    }
}

fn save_current_game(game_boy: &GameBoy) {
    if game_boy.has_battery() {
        if let Err(error) = game_boy.save_game() {
            eprintln!("Failed to save current game before loading new ROM: {error}");
        } else {
            println!("Current game saved successfully!");
        }
    }
}

fn load_rom(path: PathBuf) -> Result<GameBoy, String> {
    let rom_bytes = fs::read(&path)
        .map_err(|error| format!("Failed to read ROM at '{}': {error}", path.display()))?;

    GameBoy::from_rom_bytes(rom_bytes).map_err(|error| format!("Failed to initialize Game Boy core: {error}"))
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

    if buttons != 0 {
        eprintln!("[FE TRACE] button state=0x{buttons:02X}");
    }

    buttons
}

fn render_no_rom_screen(buffer: &mut [u32], width: usize, _height: usize) {
    buffer.fill(0xCCCCCCFF);
    draw_text(buffer, width, 12, 40, "NO ROM LOADED", 0x000000FF);
    draw_text(buffer, width, 12, 60, "PRESS L TO LOAD ROM", 0x000000FF);
    draw_text(buffer, width, 12, 80, "ESC TO QUIT", 0x000000FF);
}

fn draw_text(buffer: &mut [u32], width: usize, start_x: usize, start_y: usize, text: &str, color: u32) {
    let mut x = start_x;

    for character in text.chars() {
        let bitmap = char_bitmap(character);
        for column in 0..5 {
            let column_data = bitmap[column];
            for row in 0..7 {
                if column_data & (1 << row) != 0 {
                    let pixel_x = x + column;
                    let pixel_y = start_y + row;

                    if pixel_x < width && pixel_y < buffer.len() / width {
                        buffer[pixel_y * width + pixel_x] = color;
                    }
                }
            }
        }

        x += 6;
    }
}

fn char_bitmap(character: char) -> [u8; 5] {
    match character {
        'A' => [0x7C, 0x12, 0x11, 0x12, 0x7C],
        'C' => [0x3E, 0x41, 0x41, 0x41, 0x22],
        'D' => [0x7F, 0x41, 0x41, 0x22, 0x1C],
        'E' => [0x7F, 0x49, 0x49, 0x49, 0x41],
        'G' => [0x3E, 0x41, 0x49, 0x49, 0x3A],
        'I' => [0x00, 0x41, 0x7F, 0x41, 0x00],
        'L' => [0x7F, 0x40, 0x40, 0x40, 0x40],
        'M' => [0x7F, 0x02, 0x0C, 0x02, 0x7F],
        'N' => [0x7F, 0x06, 0x18, 0x60, 0x7F],
        'O' => [0x3E, 0x41, 0x41, 0x41, 0x3E],
        'P' => [0x7F, 0x09, 0x09, 0x09, 0x06],
        'Q' => [0x3E, 0x41, 0x51, 0x21, 0x5E],
        'R' => [0x7F, 0x09, 0x19, 0x29, 0x46],
        'S' => [0x46, 0x49, 0x49, 0x49, 0x31],
        'T' => [0x01, 0x01, 0x7F, 0x01, 0x01],
        'U' => [0x3F, 0x40, 0x40, 0x40, 0x3F],
        ' ' => [0x00, 0x00, 0x00, 0x00, 0x00],
        _ => [0x00, 0x00, 0x00, 0x00, 0x00],
    }
}
