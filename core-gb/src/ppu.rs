use crate::bus::Bus;
use crate::trace::{trace, trace_enabled};

pub const SCREEN_WIDTH: usize = 160;
pub const SCREEN_HEIGHT: usize = 144;

#[derive(Debug, Clone)]
pub struct Ppu {
    pub framebuffer: [u8; SCREEN_WIDTH * SCREEN_HEIGHT],
    cycle_counter: u32,
    scanline: u8,
    frame_counter: u32,
}

impl Default for Ppu {
    fn default() -> Self {
        Self {
            framebuffer: [0; SCREEN_WIDTH * SCREEN_HEIGHT],
            cycle_counter: 0,
            scanline: 0,
            frame_counter: 0,
        }
    }
}

impl Ppu {
    pub fn step(&mut self, cycles: u32, bus: &mut Bus) -> bool {
        self.cycle_counter = self.cycle_counter.wrapping_add(cycles);

        const SCANLINE_CYCLES: u32 = 456;
        const TOTAL_SCANLINES: u8 = 154;

        let mut frame_completed = false;
        while self.cycle_counter >= SCANLINE_CYCLES {
            self.cycle_counter -= SCANLINE_CYCLES;
            self.scanline = self.scanline.wrapping_add(1);
            if self.scanline == 144 {
                let interrupt_flags = bus.read8(0xFF0F);
                bus.write8(0xFF0F, interrupt_flags | 0x01);
                // Set VBlank flag in STAT register
                let stat = bus.read8(0xFF41);
                bus.write8(0xFF41, stat | 0x01); // Bit 0 = VBlank flag
            }
            if self.scanline >= TOTAL_SCANLINES {
                self.scanline = 0;
                self.frame_counter = self.frame_counter.wrapping_add(1);
                // Clear VBlank flag at start of new frame
                let stat = bus.read8(0xFF41);
                bus.write8(0xFF41, stat & !0x01);
                self.render_frame(bus);
                frame_completed = true;
                if trace_enabled() {
                    let lcdc = bus.read8(0xFF40);
                    let palette = bus.read8(0xFF47);
                    let stat = bus.read8(0xFF41);
                    let min_pixel = self.framebuffer.iter().copied().min().unwrap_or(0);
                    let max_pixel = self.framebuffer.iter().copied().max().unwrap_or(0);
                    trace(&format!(
                        "PPU frame: count={} scanline={} LCDC=0x{lcdc:02X} BGP=0x{palette:02X} STAT=0x{stat:02X} min_pixel={min_pixel} max_pixel={max_pixel}",
                        self.frame_counter,
                        self.scanline,
                        lcdc = lcdc,
                        palette = palette,
                        stat = stat,
                        min_pixel = min_pixel,
                        max_pixel = max_pixel,
                    ));
                }
            }
            bus.write8(0xFF44, self.scanline);
        }

        frame_completed
    }

    pub fn framebuffer(&self) -> &[u8; SCREEN_WIDTH * SCREEN_HEIGHT] {
        &self.framebuffer
    }

    fn render_frame(&mut self, bus: &Bus) {
        let lcdc = bus.read8(0xFF40);
        let bg_enabled = lcdc & 0x01 != 0;

        if !bg_enabled {
            self.framebuffer.fill(0);
            return;
        }

        let scroll_y = bus.read8(0xFF42) as usize;
        let scroll_x = bus.read8(0xFF43) as usize;
        let palette = bus.read8(0xFF47);
        let bg_map_base = if lcdc & 0x08 != 0 { 0x9C00 } else { 0x9800 };
        let tile_data_signed = lcdc & 0x10 == 0;

        for y in 0..SCREEN_HEIGHT {
            let map_y = ((y + scroll_y) & 0xFF) / 8;
            let tile_line = ((y + scroll_y) & 0x07) as u16;

            for x in 0..SCREEN_WIDTH {
                let map_x = ((x + scroll_x) & 0xFF) / 8;
                let tile_index_addr = bg_map_base + (map_y * 32 + map_x) as u16;
                let tile_index = bus.read8(tile_index_addr);

                let tile_addr = if tile_data_signed {
                    let signed_index = tile_index as i8 as i16;
                    0x9000u16.wrapping_add((signed_index * 16) as u16)
                } else {
                    0x8000u16 + u16::from(tile_index) * 16
                };

                let line_addr = tile_addr.wrapping_add(tile_line * 2);
                let b1 = bus.read8(line_addr);
                let b2 = bus.read8(line_addr.wrapping_add(1));
                let bit = 7 - ((x + scroll_x) & 0x07);
                let color_index = ((b2 >> bit) & 1) << 1 | ((b1 >> bit) & 1);
                let shade = (palette >> (color_index * 2)) & 0x03;

                self.framebuffer[y * SCREEN_WIDTH + x] = shade;
            }
        }

        // Render sprites over background
        self.render_sprites(bus, lcdc);
    }

    fn render_sprites(&mut self, bus: &Bus, lcdc: u8) {
        let sprites_enabled = lcdc & 0x02 != 0;
        if !sprites_enabled {
            return;
        }

        let sprite_height = if lcdc & 0x04 != 0 { 16 } else { 8 };
        let oam_base = 0xFE00u16;
        let palette0 = bus.read8(0xFF48);
        let palette1 = bus.read8(0xFF49);

        // Process up to 40 sprites (max 10 per scanline)
        for sprite_idx in 0..40 {
            let oam_offset = (sprite_idx * 4) as u16;
            let sprite_y = bus.read8(oam_base + oam_offset) as i16 - 16;
            let sprite_x = bus.read8(oam_base + oam_offset + 1) as i16 - 8;
            let tile_number = bus.read8(oam_base + oam_offset + 2);
            let attributes = bus.read8(oam_base + oam_offset + 3);

            let priority = attributes & 0x80 != 0;
            let y_flip = attributes & 0x40 != 0;
            let x_flip = attributes & 0x20 != 0;
            let palette_num = attributes & 0x10 != 0;
            let palette = if palette_num { palette1 } else { palette0 };

            // Render only sprites on current scanlines
            for sy in 0..sprite_height {
                let screen_y = sprite_y + sy as i16;
                if screen_y < 0 || screen_y >= SCREEN_HEIGHT as i16 {
                    continue;
                }

                let tile_y = if y_flip {
                    (sprite_height - 1 - sy) as u16
                } else {
                    sy as u16
                };

                let tile_addr = if sprite_height == 16 {
                    0x8000u16 + u16::from(tile_number & 0xFE) * 16 + tile_y * 2
                } else {
                    0x8000u16 + u16::from(tile_number) * 16 + tile_y * 2
                };

                let b1 = bus.read8(tile_addr);
                let b2 = bus.read8(tile_addr + 1);

                for sx in 0..8 {
                    let screen_x = sprite_x + sx as i16;
                    if screen_x < 0 || screen_x >= SCREEN_WIDTH as i16 {
                        continue;
                    }

                    let bit = if x_flip { sx } else { 7 - sx };
                    let color_index =
                        ((b2 >> bit) & 1) << 1 | ((b1 >> bit) & 1);

                    // Color 0 is transparent for sprites
                    if color_index == 0 {
                        continue;
                    }

                    let shade = (palette >> (color_index * 2)) & 0x03;
                    let pixel_idx = (screen_y as usize) * SCREEN_WIDTH + (screen_x as usize);

                    if priority {
                        // Behind background: only draw if background is color 0
                        let bg_color = self.framebuffer[pixel_idx];
                        if bg_color == 0 {
                            self.framebuffer[pixel_idx] = shade;
                        }
                    } else {
                        // In front of background: always draw
                        self.framebuffer[pixel_idx] = shade;
                    }
                }
            }
        }
    }
}
