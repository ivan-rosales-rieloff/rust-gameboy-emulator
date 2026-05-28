//!
//! # GBA Picture Processing Unit (PPU)
//!
//! This module implements the GBA PPU, which coordinates raster timing,
//! video modes (Modes 0-5), tiled backgrounds, sprite composition, and palettes.
//!

use crate::dma::DmaTrigger;

pub const SCREEN_WIDTH: usize = 240;
pub const SCREEN_HEIGHT: usize = 160;

/// Represents the GBA PPU state.
#[derive(Debug, Clone)]
pub struct Ppu {
    /// Screen framebuffer (240x160, XRGB values)
    pub framebuffer: Vec<u32>,
    /// Current cycle within the current scanline (0..1232)
    pub scanline_cycles: u32,
    /// Backdrop color (XRGB)
    pub backdrop_color: u32,
    /// Tracker to avoid firing HBlank trigger repeatedly on the same scanline
    pub hblank_triggered: bool,
}

impl Default for Ppu {
    fn default() -> Self {
        Self {
            framebuffer: vec![0xFF000000; SCREEN_WIDTH * SCREEN_HEIGHT],
            scanline_cycles: 0,
            backdrop_color: 0x00000000,
            hblank_triggered: false,
        }
    }
}

impl Ppu {
    /// Creates a new PPU instance.
    pub fn new() -> Self {
        Self::default()
    }

    /// Advances the PPU state by the given number of CPU cycles.
    /// Updates `DISPSTAT`, `VCOUNT` in the I/O register memory, and raises interrupts.
    /// Returns `Some(DmaTrigger)` if an event like VBlank or HBlank starts.
    pub fn tick(&mut self, cycles: u32, io: &mut [u8; 0x400], palette_ram: &[u8; 0x400], vram: &[u8], oam: &[u8; 0x400]) -> Option<DmaTrigger> {
        self.scanline_cycles += cycles;

        // Current VCOUNT is read from DISPSTAT/VCOUNT in I/O registers:
        // VCOUNT is at 0x04000006
        let mut vcount = (io[6] as u16 | ((io[7] as u16) << 8)) & 0xFF;

        let mut trigger_event = None;

        // 1 scanline = 1232 CPU cycles
        while self.scanline_cycles >= 1232 {
            self.scanline_cycles -= 1232;
            self.hblank_triggered = false;

            // Increment line
            vcount = (vcount + 1) % 228;

            // Update VCOUNT register in I/O RAM
            io[6] = vcount as u8;
            io[7] = 0;

            // Trigger line rendering when entering HDraw of active lines
            if vcount < 160 {
                self.render_scanline(vcount as usize, io, palette_ram, vram, oam);
            }

            // Frame completed when transitioning to VBlank start (line 160)
            if vcount == 160 {
                trigger_event = Some(DmaTrigger::VBlank);
            }
        }

        // Detect HBlank transition (reaches cycle 960)
        if !self.hblank_triggered && self.scanline_cycles >= 960 {
            self.hblank_triggered = true;
            if vcount < 160 && trigger_event.is_none() {
                trigger_event = Some(DmaTrigger::HBlank);
            }
        }

        // Set DISPSTAT (0x04000004) flags:
        // Bit 0: V-Blank Flag (1 = VBlank active, lines 160..227)
        // Bit 1: H-Blank Flag (1 = HBlank active, scanline_cycles >= 960)
        // Bit 2: V-Counter Match Flag (1 = VCOUNT == V-Counter Trigger)
        let dispstat_low = io[4];
        let dispstat_high = io[5];
        let mut dispstat = dispstat_low as u16 | ((dispstat_high as u16) << 8);

        // VBlank Flag
        if vcount >= 160 {
            dispstat |= 1 << 0;
            // Trigger VBlank interrupt if enabled (bit 3)
            if (dispstat & (1 << 3)) != 0 {
                self.raise_interrupt(io, 0); // Bit 0 in IF: VBlank
            }
        } else {
            dispstat &= !(1 << 0);
        }

        // HBlank Flag: Active during the last 272 cycles of the 1232 scanline cycles
        if self.scanline_cycles >= 960 {
            dispstat |= 1 << 1;
            // Trigger HBlank interrupt if enabled (bit 4)
            if (dispstat & (1 << 4)) != 0 {
                self.raise_interrupt(io, 1); // Bit 1 in IF: HBlank
            }
        } else {
            dispstat &= !(1 << 1);
        }

        // VCOUNT Match Flag: Compare vcount to bit 8-15 of DISPSTAT
        let vcount_trigger = (dispstat >> 8) & 0xFF;
        if vcount == vcount_trigger {
            dispstat |= 1 << 2;
            // Trigger VCOUNT Match interrupt if enabled (bit 5)
            if (dispstat & (1 << 5)) != 0 {
                self.raise_interrupt(io, 2); // Bit 2 in IF: VCOUNT Match
            }
        } else {
            dispstat &= !(1 << 2);
        }

        // Write back DISPSTAT
        io[4] = dispstat as u8;
        io[5] = (dispstat >> 8) as u8;

        trigger_event
    }

    /// Raises an interrupt by setting the corresponding bit in the IF register (0x04000202).
    fn raise_interrupt(&self, io: &mut [u8; 0x400], interrupt_bit: u8) {
        let current_if = io[0x202] as u16 | ((io[0x203] as u16) << 8);
        let new_if = current_if | (1 << interrupt_bit);
        io[0x202] = new_if as u8;
        io[0x203] = (new_if >> 8) as u8;
    }

    /// Converts a 15-bit BGR555 color value into a 32-bit XRGB color value.
    #[inline]
    fn convert_bgr555_to_xrgb(color: u16) -> u32 {
        let r = ((color & 0x1F) as u32) << 3;
        let g = (((color >> 5) & 0x1F) as u32) << 3;
        let b = (((color >> 10) & 0x1F) as u32) << 3;
        // Stretch to 8-bit
        let r = r | (r >> 5);
        let g = g | (g >> 5);
        let b = b | (b >> 5);
        (r << 16) | (g << 8) | b
    }

    /// Renders a single active scanline.
    fn render_scanline(&mut self, y: usize, io: &mut [u8; 0x400], palette_ram: &[u8; 0x400], vram: &[u8], oam: &[u8; 0x400]) {
        let dispcnt = io[0] as u16 | ((io[1] as u16) << 8);
        let mode = dispcnt & 0x7;

        // Fetch backdrop color (first entry of BG palette)
        let backdrop_raw = palette_ram[0] as u16 | ((palette_ram[1] as u16) << 8);
        self.backdrop_color = Self::convert_bgr555_to_xrgb(backdrop_raw);

        // Pre-fill the scanline with backdrop color
        for x in 0..SCREEN_WIDTH {
            self.framebuffer[y * SCREEN_WIDTH + x] = self.backdrop_color;
        }

        match mode {
            0 => {
                // Mode 0: Tiled BG0, BG1, BG2, BG3
                self.render_tiled_backgrounds(y, dispcnt, io, palette_ram, vram);
            }
            1 => {
                // Mode 1: Tiled BG0, BG1 + Affine BG2
                self.render_tiled_backgrounds(y, dispcnt, io, palette_ram, vram);
            }
            2 => {
                // Mode 2: Affine BG2, BG3
                self.render_tiled_backgrounds(y, dispcnt, io, palette_ram, vram);
            }
            3 => {
                // Mode 3: 240x160 16-bit direct bitmap in VRAM
                for x in 0..SCREEN_WIDTH {
                    let pixel_index = y * SCREEN_WIDTH + x;
                    let vram_offset = pixel_index * 2;
                    if vram_offset + 1 < vram.len() {
                        let color_raw = vram[vram_offset] as u16 | ((vram[vram_offset + 1] as u16) << 8);
                        self.framebuffer[pixel_index] = Self::convert_bgr555_to_xrgb(color_raw);
                    }
                }
            }
            4 => {
                // Mode 4: 240x160 8-bit palettized double-buffered bitmap
                let page_select = (dispcnt & (1 << 4)) != 0;
                let base = if page_select { 0xA000 } else { 0x0000 };
                for x in 0..SCREEN_WIDTH {
                    let pixel_index = y * SCREEN_WIDTH + x;
                    let vram_offset = base + pixel_index;
                    if vram_offset < vram.len() {
                        let palette_idx = vram[vram_offset];
                        let pal_offset = (palette_idx as usize) * 2;
                        if pal_offset + 1 < palette_ram.len() {
                            let color_raw = palette_ram[pal_offset] as u16 | ((palette_ram[pal_offset + 1] as u16) << 8);
                            self.framebuffer[pixel_index] = Self::convert_bgr555_to_xrgb(color_raw);
                        }
                    }
                }
            }
            5 => {
                // Mode 5: 160x128 16-bit direct bitmap, centered/padded
                let page_select = (dispcnt & (1 << 4)) != 0;
                let base = if page_select { 0xA000 } else { 0x0000 };
                if y >= 16 && y < 144 {
                    let y_local = y - 16;
                    for x in 40..200 {
                        let x_local = x - 40;
                        let pixel_index = y_local * 160 + x_local;
                        let vram_offset = base + pixel_index * 2;
                        if vram_offset + 1 < vram.len() {
                            let color_raw = vram[vram_offset] as u16 | ((vram[vram_offset + 1] as u16) << 8);
                            self.framebuffer[y * SCREEN_WIDTH + x] = Self::convert_bgr555_to_xrgb(color_raw);
                        }
                    }
                }
            }
            _ => {}
        }

        // Render Sprites (OBJ layer)
        if (dispcnt & (1 << 12)) != 0 {
            self.render_sprites(y, dispcnt, palette_ram, vram, oam);
        }
    }

    /// Renders tiled background layers (BG0-BG3) for Mode 0, 1, 2.
    fn render_tiled_backgrounds(&mut self, y: usize, dispcnt: u16, io: &[u8; 0x400], palette_ram: &[u8; 0x400], vram: &[u8]) {
        let mut layers = Vec::new();
        for bg in 0..4 {
            let bg_enable = (dispcnt & (1 << (8 + bg))) != 0;
            if bg_enable {
                let bg_cnt_offset = 0x08 + bg * 2;
                let bgcnt = io[bg_cnt_offset] as u16 | ((io[bg_cnt_offset + 1] as u16) << 8);
                let priority = bgcnt & 0x3;
                layers.push((bg, priority, bgcnt));
            }
        }

        layers.sort_by(|a, b| b.1.cmp(&a.1));

        for &(bg, _, bgcnt) in &layers {
            let char_base = ((bgcnt >> 2) & 0x3) as u32 * 16384;
            let screen_base = ((bgcnt >> 8) & 0x1F) as u32 * 2048;
            let screen_size = (bgcnt >> 14) & 0x3;
            let color_256 = (bgcnt & (1 << 7)) != 0;

            let (bg_width, bg_height) = match screen_size {
                0 => (256, 256),
                1 => (512, 256),
                2 => (256, 512),
                3 => (512, 512),
                _ => (256, 256),
            };

            let hofs_offset = 0x10 + bg * 4;
            let vofs_offset = 0x12 + bg * 4;
            let hofs = (io[hofs_offset] as u16 | ((io[hofs_offset + 1] as u16) << 8)) & 0x1FF;
            let vofs = (io[vofs_offset] as u16 | ((io[vofs_offset + 1] as u16) << 8)) & 0x1FF;

            for x in 0..SCREEN_WIDTH {
                let bg_x = (x as u32 + hofs as u32) % bg_width;
                let bg_y = (y as u32 + vofs as u32) % bg_height;

                let tile_col = bg_x / 8;
                let tile_row = bg_y / 8;

                let mut map_col = tile_col;
                let mut map_row = tile_row;
                let mut map_block = screen_base;

                match screen_size {
                    1 => {
                        if map_col >= 32 {
                            map_col -= 32;
                            map_block += 2048;
                        }
                    }
                    2 => {
                        if map_row >= 32 {
                            map_row -= 32;
                            map_block += 2048;
                        }
                    }
                    3 => {
                        if map_col >= 32 {
                            map_col -= 32;
                            map_block += 2048;
                        }
                        if map_row >= 32 {
                            map_row -= 32;
                            map_block += 4096;
                        }
                    }
                    _ => {}
                }

                let map_addr = map_block + (map_row * 32 + map_col) * 2;
                if map_addr + 1 < vram.len() as u32 {
                    let map_entry = vram[map_addr as usize] as u16 | ((vram[map_addr as usize + 1] as u16) << 8);
                    let tile_idx = map_entry & 0x3FF;
                    let h_flip = (map_entry & (1 << 10)) != 0;
                    let v_flip = (map_entry & (1 << 11)) != 0;
                    let palette_bank = ((map_entry >> 12) & 0x0F) as u8;

                    let mut px = bg_x % 8;
                    let mut py = bg_y % 8;
                    if h_flip { px = 7 - px; }
                    if v_flip { py = 7 - py; }

                    let mut color_idx = 0u8;
                    if !color_256 {
                        let tile_offset = char_base + tile_idx as u32 * 32 + py * 4 + px / 2;
                        if tile_offset < vram.len() as u32 {
                            let byte = vram[tile_offset as usize];
                            color_idx = if px % 2 == 0 { byte & 0x0F } else { byte >> 4 };
                        }
                    } else {
                        let tile_offset = char_base + tile_idx as u32 * 64 + py * 8 + px;
                        if tile_offset < vram.len() as u32 {
                            color_idx = vram[tile_offset as usize];
                        }
                    }

                    if color_idx != 0 {
                        let pal_offset = if !color_256 {
                            (palette_bank as usize * 16 + color_idx as usize) * 2
                        } else {
                            (color_idx as usize) * 2
                        };

                        if pal_offset + 1 < palette_ram.len() {
                            let color_raw = palette_ram[pal_offset] as u16 | ((palette_ram[pal_offset + 1] as u16) << 8);
                            self.framebuffer[y * SCREEN_WIDTH + x] = Self::convert_bgr555_to_xrgb(color_raw);
                        }
                    }
                }
            }
        }
    }

    /// Renders sprites (Object layer) for the current scanline.
    fn render_sprites(&mut self, y: usize, dispcnt: u16, palette_ram: &[u8; 0x400], vram: &[u8], oam: &[u8; 0x400]) {
        let is_1d_mapping = (dispcnt & (1 << 6)) != 0;

        for i in (0..128).rev() {
            let oam_offset = i * 8;
            let attr0 = oam[oam_offset] as u16 | ((oam[oam_offset + 1] as u16) << 8);
            let attr1 = oam[oam_offset + 2] as u16 | ((oam[oam_offset + 3] as u16) << 8);
            let attr2 = oam[oam_offset + 4] as u16 | ((oam[oam_offset + 5] as u16) << 8);

            let mut sprite_y = (attr0 & 0xFF) as i16;
            if sprite_y >= 160 {
                sprite_y -= 256;
            }

            let rot_scale = (attr0 & (1 << 8)) != 0;
            let disabled = !rot_scale && (attr0 & (1 << 9)) != 0;
            if disabled {
                continue;
            }

            let shape = (attr0 >> 14) & 0x3;
            let size = (attr1 >> 14) & 0x3;

            let (sprite_w, sprite_h) = match (shape, size) {
                (0, 0) => (8, 8),
                (0, 1) => (16, 16),
                (0, 2) => (32, 32),
                (0, 3) => (64, 64),
                (1, 0) => (16, 8),
                (1, 1) => (32, 8),
                (1, 2) => (32, 16),
                (1, 3) => (64, 32),
                (2, 0) => (8, 16),
                (2, 1) => (8, 32),
                (2, 2) => (16, 32),
                (2, 3) => (32, 64),
                _ => (8, 8),
            };

            let y_in_sprite = y as i16 - sprite_y;
            if y_in_sprite < 0 || y_in_sprite >= sprite_h as i16 {
                continue;
            }

            let mut sprite_x = (attr1 & 0x1FF) as i16;
            if sprite_x >= 246 {
                sprite_x -= 512;
            }

            let h_flip = !rot_scale && (attr1 & (1 << 12)) != 0;
            let v_flip = !rot_scale && (attr1 & (1 << 13)) != 0;

            let color_256 = (attr0 & (1 << 13)) != 0;
            let tile_idx = attr2 & 0x3FF;
            let palette_bank = ((attr2 >> 12) & 0x0F) as u8;

            let mut py = y_in_sprite;
            if v_flip {
                py = sprite_h as i16 - 1 - py;
            }

            for sx in 0..sprite_w {
                let x = sprite_x + sx as i16;
                if x < 0 || x >= SCREEN_WIDTH as i16 {
                    continue;
                }

                let mut px = sx;
                if h_flip {
                    px = sprite_w - 1 - px;
                }

                let tile_x = px / 8;
                let tile_y = py / 8;
                let local_px = px % 8;
                let local_py = py % 8;

                let char_offset = if is_1d_mapping {
                    let tiles_per_row = sprite_w as u32 / 8;
                    let tile_delta = (tile_y as u32 * tiles_per_row + tile_x as u32) * (if color_256 { 2 } else { 1 });
                    tile_idx as u32 + tile_delta
                } else {
                    tile_idx as u32 + tile_y as u32 * 32 + tile_x as u32 * (if color_256 { 2 } else { 1 })
                };

                let mut color_idx = 0u8;
                if !color_256 {
                    let tile_addr = 0x10000 + char_offset * 32 + local_py as u32 * 4 + local_px as u32 / 2;
                    if tile_addr < vram.len() as u32 {
                        let byte = vram[tile_addr as usize];
                        color_idx = if local_px % 2 == 0 { byte & 0x0F } else { byte >> 4 };
                    }
                } else {
                    let tile_addr = 0x10000 + char_offset * 32 + local_py as u32 * 8 + local_px as u32;
                    if tile_addr < vram.len() as u32 {
                        color_idx = vram[tile_addr as usize];
                    }
                }

                if color_idx != 0 {
                    let pal_offset = if !color_256 {
                        0x200 + (palette_bank as usize * 16 + color_idx as usize) * 2
                    } else {
                        0x200 + (color_idx as usize) * 2
                    };

                    if pal_offset + 1 < palette_ram.len() {
                        let color_raw = palette_ram[pal_offset] as u16 | ((palette_ram[pal_offset + 1] as u16) << 8);
                        self.framebuffer[y * SCREEN_WIDTH + x as usize] = Self::convert_bgr555_to_xrgb(color_raw);
                    }
                }
            }
        }
    }
}
