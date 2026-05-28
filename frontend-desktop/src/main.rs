use std::env;
use std::fs;
use std::path::PathBuf;
use std::process;

use core_gb::GameBoy;
use core_gba::GameBoyAdvance;
use minifb::{Key, Scale, Window, WindowOptions};
use rfd::FileDialog;

const PALETTE: [u32; 4] = [0x00FFFFFF, 0x00AAAAAA, 0x00555555, 0x00000000];

// Joypad button bits (active high in our representation)
const BTN_A: u8 = 0x01;
const BTN_B: u8 = 0x02;
const BTN_SELECT: u8 = 0x04;
const BTN_START: u8 = 0x08;
const BTN_RIGHT: u8 = 0x10;
const BTN_LEFT: u8 = 0x20;
const BTN_UP: u8 = 0x40;
const BTN_DOWN: u8 = 0x80;

enum EmulatorCore {
    GameBoy(GameBoy),
    GameBoyAdvance(GameBoyAdvance),
}

impl EmulatorCore {
    fn screen_dimensions(&self) -> (usize, usize) {
        match self {
            Self::GameBoy(_) => (160, 144),
            Self::GameBoyAdvance(_) => (240, 160),
        }
    }

    fn title(&self) -> String {
        match self {
            Self::GameBoy(gb) => gb.title().to_string(),
            Self::GameBoyAdvance(gba) => gba.title().to_string(),
        }
    }

    fn run_frame(&mut self) -> Result<(), String> {
        match self {
            Self::GameBoy(gb) => gb.run_frame().map_err(|e| format!("Game Boy execution error: {:?}", e)),
            Self::GameBoyAdvance(gba) => gba.run_frame().map_err(|_| "Game Boy Advance execution error".to_string()),
        }
    }

    fn set_button_state(&mut self, buttons: u8) {
        match self {
            Self::GameBoy(gb) => gb.set_button_state(buttons),
            Self::GameBoyAdvance(gba) => gba.set_button_state(buttons),
        }
    }

    fn take_audio_samples(&mut self) -> Vec<f32> {
        match self {
            Self::GameBoy(gb) => gb.take_audio_samples(),
            Self::GameBoyAdvance(gba) => gba.take_audio_samples(),
        }
    }

    fn has_battery(&self) -> bool {
        match self {
            Self::GameBoy(gb) => gb.has_battery(),
            Self::GameBoyAdvance(gba) => gba.has_battery(),
        }
    }

    fn save_game(&self) -> Result<(), String> {
        match self {
            Self::GameBoy(gb) => gb.save_game().map_err(|e| format!("Save failed: {:?}", e)),
            Self::GameBoyAdvance(gba) => gba.save_game().map_err(|_| "Save failed".to_string()),
        }
    }

    fn render_to_buffer(&self, dest: &mut [u32]) {
        match self {
            Self::GameBoy(gb) => {
                for (pixel_index, &pixel_value) in gb.framebuffer().iter().enumerate() {
                    dest[pixel_index] = PALETTE[pixel_value as usize];
                }
            }
            Self::GameBoyAdvance(gba) => {
                dest.copy_from_slice(gba.framebuffer());
            }
        }
    }
}

fn main() {
    let mut core = env::args().nth(1).map(PathBuf::from).map(load_rom).transpose().unwrap_or_else(|error| {
        eprintln!("{error}");
        process::exit(1);
    });

    let (mut width, mut height) = if let Some(c) = &core {
        c.screen_dimensions()
    } else {
        (160, 144)
    };

    let mut window = Window::new(
        if let Some(c) = &core { &c.title() } else { "Game Boy / GBA Emulator" },
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

    window.set_target_fps(60);

    // Initialize audio playback with rodio
    let audio_output = rodio::OutputStream::try_default().ok().and_then(|(stream, handle)| {
        rodio::Sink::try_new(&handle).ok().map(|sink| (stream, sink))
    });

    let mut buffer = vec![0u32; width * height];

    while window.is_open() && !window.is_key_down(Key::Escape) {
        if window.is_key_pressed(Key::L, minifb::KeyRepeat::No) {
            if let Some(game) = &core {
                save_current_game(game);
            }

            if let Some(path) = FileDialog::new()
                .add_filter("Game ROM", &[
                    "gb",
                    "gbc",
                    "gba",
                ])
                .pick_file()
            {
                match load_rom(path) {
                    Ok(loaded_core) => {
                        let dims = loaded_core.screen_dimensions();
                        width = dims.0;
                        height = dims.1;
                        buffer = vec![0u32; width * height];
                        
                        // Recreate the window with the new core's dimensions
                        window = Window::new(
                            &loaded_core.title(),
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
                        window.set_target_fps(60);

                        core = Some(loaded_core);
                    }
                    Err(error) => {
                        eprintln!("Failed to load ROM: {error}");
                    }
                }
            }
        }

        if let Some(active_core) = &mut core {
            let buttons = read_buttons(&window);
            active_core.set_button_state(buttons);

            if let Err(error) = active_core.run_frame() {
                eprintln!("Emulation error: {error}");
                process::exit(1);
            }

            active_core.render_to_buffer(&mut buffer);

            // Play audio samples
            let samples = active_core.take_audio_samples();
            if let Some((_stream, sink)) = &audio_output {
                if !samples.is_empty() {
                    let source = rodio::buffer::SamplesBuffer::new(2, 44100, samples);
                    sink.append(source);
                }
            }
        } else {
            render_no_rom_screen(&mut buffer, width, height);
        }

        window.update_with_buffer(&buffer, width, height).unwrap();
    }

    if let Some(active_core) = &core {
        if active_core.has_battery() {
            if let Err(error) = active_core.save_game() {
                eprintln!("Failed to save game: {error}");
            } else {
                println!("Game saved successfully!");
            }
        }
    }
}

fn save_current_game(core: &EmulatorCore) {
    if core.has_battery() {
        if let Err(error) = core.save_game() {
            eprintln!("Failed to save current game before loading new ROM: {error}");
        } else {
            println!("Current game saved successfully!");
        }
    }
}

fn load_rom(path: PathBuf) -> Result<EmulatorCore, String> {
    let extension = path.extension()
        .and_then(|ext| ext.to_str())
        .map(|s| s.to_lowercase());

    let rom_bytes = fs::read(&path)
        .map_err(|error| format!("Failed to read ROM at '{}': {error}", path.display()))?;

    match extension.as_deref() {
        Some("gba") => {
            let core = GameBoyAdvance::from_rom_bytes(rom_bytes)
                .map_err(|error| format!("Failed to initialize Game Boy Advance core: {:?}", error))?;
            Ok(EmulatorCore::GameBoyAdvance(core))
        }
        Some("gb") | Some("gbc") | _ => {
            let core = GameBoy::from_rom_bytes(rom_bytes)
                .map_err(|error| format!("Failed to initialize Game Boy core: {:?}", error))?;
            Ok(EmulatorCore::GameBoy(core))
        }
    }
}

fn read_buttons(window: &Window) -> u8 {
    let mut buttons = 0u8;

    if window.is_key_down(Key::Z) {
        buttons |= BTN_A;
    }
    if window.is_key_down(Key::X) {
        buttons |= BTN_B;
    }
    if window.is_key_down(Key::Space) {
        buttons |= BTN_SELECT;
    }
    if window.is_key_down(Key::Enter) {
        buttons |= BTN_START;
    }
    if window.is_key_down(Key::Right) {
        buttons |= BTN_RIGHT;
    }
    if window.is_key_down(Key::Left) {
        buttons |= BTN_LEFT;
    }
    if window.is_key_down(Key::Up) {
        buttons |= BTN_UP;
    }
    if window.is_key_down(Key::Down) {
        buttons |= BTN_DOWN;
    }

    if buttons != 0 && trace_enabled() {
        eprintln!("[FE TRACE] button state=0x{buttons:02X}");
    }

    buttons
}

fn trace_enabled() -> bool {
    env::args().any(|arg| arg == "--uitrace" || std::env::var_os("GB_TRACE").is_some())
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
