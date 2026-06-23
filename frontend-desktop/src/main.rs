use std::env;
use std::fs;
use std::path::PathBuf;
use std::process;

use core_gb::GameBoy;
use minifb::{Key, Scale, Window, WindowOptions};
use rfd::FileDialog;

mod network;
use network::NetworkSettings;

use core_gb::printer::{GbPrinter, PrinterImage};
use std::sync::mpsc;
use std::time::Duration;

fn save_printer_image(img: &PrinterImage) -> Result<(), Box<dyn std::error::Error>> {
    let output_dir = std::path::Path::new("output");
    if !output_dir.exists() {
        std::fs::create_dir_all(output_dir)?;
    }
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs();
    let path = output_dir.join(format!("print_{}.png", timestamp));
    
    let mut image_buf = image::ImageBuffer::new(img.width as u32, img.height as u32);
    for (x, y, pixel) in image_buf.enumerate_pixels_mut() {
        let luma = img.data[(y as usize) * img.width + (x as usize)];
        *pixel = image::Luma([luma]);
    }
    image_buf.save(&path)?;
    println!("Printer image saved to {}", path.display());
    Ok(())
}


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
    let mut game_boy = env::args()
        .nth(1)
        .map(PathBuf::from)
        .map(load_rom)
        .transpose()
        .unwrap_or_else(|error| {
            eprintln!("{error}");
            process::exit(1);
        });

    let (width, height) = GameBoy::screen_dimensions();
    let mut current_scale_num = 4;
    let mut current_fast_forward = false;
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

    window.set_target_fps(60);

    let mut network_settings = NetworkSettings::default();
    let mut network_menu_open = false;
    let mut link_active = false;

    // Initialize audio playback with rodio
    let audio_output = rodio::OutputStream::try_default()
        .ok()
        .and_then(|(stream, handle)| {
            rodio::Sink::try_new(&handle)
                .ok()
                .map(|sink| (stream, sink))
        });

    let mut buffer = vec![0u32; width * height];
    let (printer_tx, printer_rx) = mpsc::channel::<PrinterImage>();
    let mut printer_active = false;
    
    if let Some(game) = &mut game_boy {
        // Printer is not connected by default to prevent breaking games that probe the serial port
    }

    while window.is_open() && !window.is_key_down(Key::Escape) {
        if window.is_key_pressed(Key::L, minifb::KeyRepeat::No) {
            if let Some(game) = &game_boy {
                save_current_game(game);
            }

            if let Some(path) = FileDialog::new()
                .add_filter("Game Boy ROM", &["gb", "gbc"])
                .pick_file()
            {
                match load_rom(path) {
                    Ok(mut loaded_game) => {
                        if printer_active {
                            let printer = GbPrinter::new(Some(printer_tx.clone()));
                            loaded_game.bus.connect_link(Box::new(printer));
                        }
                        game_boy = Some(loaded_game);
                        link_active = false;
                        network_settings.disconnect();
                    }
                    Err(error) => {
                        eprintln!("Failed to load ROM: {error}");
                    }
                }
            }
        }

        if window.is_key_pressed(Key::S, minifb::KeyRepeat::No) {
            if let Some(game) = &game_boy {
                if let Some(path) = FileDialog::new()
                    .add_filter("Game Boy State", &["state", "sav"])
                    .save_file()
                {
                    if let Err(error) = game.save_state(path) {
                        eprintln!("Failed to save state: {error}");
                    }
                }
            }
        }

        // Save cartridge-backed game (battery save) to chosen file
        if window.is_key_pressed(Key::G, minifb::KeyRepeat::No) {
            if let Some(game) = &game_boy {
                if let Some(path) = FileDialog::new()
                    .add_filter("Game Save", &["sav"])
                    .save_file()
                {
                    if let Err(error) = game.save_game_to(path) {
                        eprintln!("Failed to save game: {error}");
                    } else {
                        println!("Game save written to chosen path");
                    }
                }
            }
        }

        // Load a chosen .sav into the cartridge RAM
        if window.is_key_pressed(Key::U, minifb::KeyRepeat::No) {
            if let Some(game) = &mut game_boy {
                if let Some(path) = FileDialog::new().add_filter("Game Save", &["sav"]).pick_file() {
                    if let Err(error) = game.load_game_from(path) {
                        eprintln!("Failed to load game save: {error}");
                    } else {
                        println!("Game save loaded into cartridge RAM");
                    }
                }
            }
        }

        if window.is_key_pressed(Key::O, minifb::KeyRepeat::No) {
            if let Some(path) = FileDialog::new()
                .add_filter("Game Boy State", &["state", "sav"])
                .pick_file()
            {
                match GameBoy::load_state(path) {
                    Ok(mut state) => {
                        if printer_active {
                            let printer = GbPrinter::new(Some(printer_tx.clone()));
                            state.bus.connect_link(Box::new(printer));
                        }
                        game_boy = Some(Box::new(state));
                        link_active = false;
                        network_settings.disconnect();
                    }
                    Err(error) => {
                        eprintln!("Failed to load state: {error}");
                    }
                }
            }
        }

        if window.is_key_pressed(Key::N, minifb::KeyRepeat::No) {
            network_menu_open = !network_menu_open;
        }

        if network_menu_open {
            if window.is_key_pressed(Key::M, minifb::KeyRepeat::No) {
                network_settings.toggle_mode();
                println!("Network mode: {}", network_settings.mode.label());
            }
            if window.is_key_pressed(Key::H, minifb::KeyRepeat::No) {
                network_settings.cycle_host();
            }
            if window.is_key_pressed(Key::Up, minifb::KeyRepeat::No) {
                network_settings.change_port(1);
            }
            if window.is_key_pressed(Key::Down, minifb::KeyRepeat::No) {
                network_settings.change_port(-1);
            }
            if window.is_key_pressed(Key::C, minifb::KeyRepeat::No) {
                if let Some(game_boy) = &mut game_boy {
                    if link_active {
                        game_boy.disconnect_link();
                        link_active = false;
                        network_settings.disconnect();
                    } else {
                        match network_settings.connect() {
                            Ok(Some(endpoint)) => {
                                game_boy.connect_link(endpoint);
                                link_active = true;
                            }
                            Ok(None) => {
                                // Wait for server accept if in server mode.
                            }
                            Err(error) => {
                                eprintln!("Network error: {error}");
                            }
                        }
                    }
                }
            }
        }

        if window.is_key_pressed(Key::P, minifb::KeyRepeat::No) {
            printer_active = !printer_active;
            if let Some(game) = &mut game_boy {
                if printer_active {
                    let printer = GbPrinter::new(Some(printer_tx.clone()));
                    game.bus.connect_link(Box::new(printer));
                    println!("Printer connected!");
                } else {
                    game.bus.disconnect_link();
                    println!("Printer disconnected!");
                }
            }
        }

        // --- Fast Forward & Scale Toggling ---
        let fast_forward = window.is_key_down(Key::F12);
        if fast_forward != current_fast_forward {
            current_fast_forward = fast_forward;
            if fast_forward {
                window.set_target_fps(0);
            } else {
                window.set_target_fps(60);
            }
        }

        let mut new_scale_num = current_scale_num;
        if window.is_key_pressed(Key::Key1, minifb::KeyRepeat::No) { new_scale_num = 1; }
        if window.is_key_pressed(Key::Key2, minifb::KeyRepeat::No) { new_scale_num = 2; }
        if window.is_key_pressed(Key::Key3, minifb::KeyRepeat::No) { new_scale_num = 4; }
        if window.is_key_pressed(Key::Key4, minifb::KeyRepeat::No) { new_scale_num = 8; }

        if new_scale_num != current_scale_num {
            current_scale_num = new_scale_num;
            let scale_val = match current_scale_num {
                1 => Scale::X1,
                2 => Scale::X2,
                8 => Scale::X8,
                _ => Scale::X4,
            };
            let mut new_window = Window::new(
                "Game Boy Emulator",
                width,
                height,
                WindowOptions {
                    scale: scale_val,
                    ..WindowOptions::default()
                },
            ).unwrap();
            if fast_forward {
                new_window.set_target_fps(0);
            } else {
                new_window.set_target_fps(60);
            }
            window = new_window;
        }

        // --- Printer checking ---
        if let Ok(img) = printer_rx.try_recv() {
            if let Err(e) = save_printer_image(&img) {
                eprintln!("Failed to save printer image: {}", e);
            }
        }

        if let Some(game_boy) = &mut game_boy {
            let buttons = read_buttons(&window);

            if !link_active {
                if let Ok(Some(endpoint)) = network_settings.poll_server() {
                    game_boy.connect_link(endpoint);
                    link_active = true;
                }
            }

            game_boy.set_button_state(buttons);

            if let Err(error) = game_boy.run_frame() {
                eprintln!("Emulation error: {error}");
                process::exit(1);
            }

            buffer.copy_from_slice(game_boy.framebuffer());

            // Play audio samples
            let samples = game_boy.take_audio_samples();
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

fn load_rom(path: PathBuf) -> Result<Box<GameBoy>, String> {
    let rom_bytes = fs::read(&path)
        .map_err(|error| format!("Failed to read ROM at '{}': {error}", path.display()))?;

    GameBoy::from_rom_bytes(rom_bytes)
        .map(Box::new)
        .map_err(|error| format!("Failed to initialize Game Boy core: {error}"))
}

fn read_buttons(window: &Window) -> u8 {
    let mut buttons = 0u8;

    // Use is_key_down for immediate raw key state.
    // is_key_pressed uses OS key-repeat delay (~500ms initial),
    // causing noticeable input lag in an emulator.
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

fn draw_text(
    buffer: &mut [u32],
    width: usize,
    start_x: usize,
    start_y: usize,
    text: &str,
    color: u32,
) {
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
