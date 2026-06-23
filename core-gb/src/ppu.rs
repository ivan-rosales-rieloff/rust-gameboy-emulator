//!
//! # Game Boy Picture Processing Unit (PPU)
//!
//! The PPU is responsible for generating the Game Boy's video output. It renders
//! graphics by combining background tiles, window tiles, and sprites (OAM objects).
//!
//! ## PPU Architecture Overview
//!
//! The Game Boy PPU operates on a scanline-based rendering system:
//! - **Resolution**: 160x144 pixels (20x18 tiles of 8x8 pixels)
//! - **Frame Rate**: 59.73 FPS (4.194304 MHz / 70224 cycles per frame)
//! - **Scanlines**: 154 total (144 visible + 10 VBlank lines)
//! - **Cycles per scanline**: 456 cycles (109,824 cycles per frame)
//!
//! ## Rendering Pipeline
//!
//! 1. **Background Layer**: Tile-based background using tile maps and patterns
//! 2. **Window Layer**: Optional overlay window with its own tile map
//! 3. **Sprite Layer**: Up to 40 sprites (10 per scanline max) with priority
//! 4. **Color Palette**: 4 shades of gray applied to all layers
//!
//! ## Tile System
//!
//! - **Tile Size**: 8x8 pixels, stored as 16 bytes (2 bytes per row)
//! - **Pixel Format**: 2 bits per pixel (4 colors: 0, 1, 2, 3)
//! - **Tile Data**: 0x8000-0x97FF (unsigned) or 0x8800-0x97FF (signed)
//! - **Tile Maps**: 0x9800-0x9BFF (background) and 0x9C00-0x9FFF (window)
//!
//! ## LCD Control Register (LCDC - 0xFF40)
//!
//! ```text
//! Bit 7: LCD Enable (0=Off, 1=On)
//! Bit 6: Window Tile Map (0=0x9800, 1=0x9C00)
//! Bit 5: Window Enable (0=Off, 1=On)
//! Bit 4: Tile Data Select (0=0x8800, 1=0x8000)
//! Bit 3: BG Tile Map (0=0x9800, 1=0x9C00)
//! Bit 2: Sprite Size (0=8x8, 1=8x16)
//! Bit 1: Sprite Enable (0=Off, 1=On)
//! Bit 0: BG Enable (0=Off, 1=On)
//! ```
//!
//! ## LCD Status Register (STAT - 0xFF41)
//!
//! ```text
//! Bit 6: LYC=LY Interrupt Enable
//! Bit 5: Mode 2 OAM Interrupt Enable
//! Bit 4: Mode 1 VBlank Interrupt Enable
//! Bit 3: Mode 0 HBlank Interrupt Enable
//! Bit 2: LYC=LY Flag (1 when LY==LYC)
//! Bit 1-0: Mode (0=HBlank, 1=VBlank, 2=OAM Search, 3=Transfer)
//! ```
//!
//! ## Color Palettes
//!
//! The Game Boy uses indexed colors with 4 shades:
//! - **BGP (0xFF47)**: Background palette mapping (4 colors)
//! - **OBP0/1 (0xFF48/49)**: Sprite palette mappings (4 colors each)
//!
//! Each palette register maps color indices (0-3) to shades (0-3).
//!
//! ## Sprite (OAM) System
//!
//! - **OAM Size**: 160 bytes (40 sprites × 4 bytes each)
//! - **Sprite Attributes**:
//!   - Byte 0: Y position (top edge, 0-255)
//!   - Byte 1: X position (left edge, 0-255)
//!   - Byte 2: Tile number (0-255)
//!   - Byte 3: Attributes (priority, flip, palette)
//!
//! ## Timing and Interrupts
//!
//! The PPU generates interrupts at specific times:
//! - **VBlank**: Scanline 144, triggers VBlank interrupt
//! - **HBlank**: End of each visible scanline
//! - **OAM Search**: Beginning of each scanline (mode 2)
//! - **Transfer**: Pixel transfer period (mode 3)
//!
//! ## Rust Implementation Notes
//!
//! - Uses a fixed-size framebuffer array for performance
//! - Cycle-accurate timing ensures proper synchronization with CPU
//! - Sprite rendering handles priority and transparency correctly
//! - Scrolling and windowing implemented with wraparound logic

use crate::bus::Bus;
use crate::trace::{trace, trace_enabled};
use crate::serde_array;
use serde::{Deserialize, Serialize};

/// Game Boy screen dimensions in pixels
pub const SCREEN_WIDTH: usize = 160;
pub const SCREEN_HEIGHT: usize = 144;

/// Game Boy Picture Processing Unit (PPU) emulator.
///
/// The PPU handles all graphics rendering for the Game Boy, including:
/// - Background and window tile rendering
/// - Sprite (OAM object) rendering
/// - Color palette application
/// - LCD timing and interrupt generation
/// - Framebuffer management
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Ppu {
    /// The final rendered image (160x144 pixels, 32-bit ARGB/XRGB color per pixel)
    pub framebuffer: Vec<u32>,

    /// Metadata about background pixels (color index, priority) for sprite rendering
    pub bg_pixel_info: Vec<u8>,

    /// Cycle counter for timing scanline progression
    cycle_counter: u32,

    /// Current scanline being rendered (0-153)
    /// 0-143: Visible scanlines
    /// 144-153: VBlank period
    scanline: u8,

    /// Total frames rendered (for debugging/performance tracking)
    frame_counter: u32,
}

impl Default for Ppu {
    /// Creates a PPU in default state (LCD off, blank screen)
    fn default() -> Self {
        Self {
            framebuffer: vec![0xFFFFFFFF; SCREEN_WIDTH * SCREEN_HEIGHT], // Initialize to white
            bg_pixel_info: vec![0; SCREEN_WIDTH * SCREEN_HEIGHT],
            cycle_counter: 0,
            scanline: 0,
            frame_counter: 0,
        }
    }
}

impl Ppu {
    /// Advances the PPU by the specified number of CPU cycles.
    ///
    /// The PPU runs synchronized with the CPU at a 1:1 cycle ratio.
    /// Each scanline takes 456 cycles, and a complete frame takes 70,224 cycles.
    ///
    /// # Arguments
    /// * `cycles` - Number of CPU cycles to advance
    /// * `bus` - Memory bus for register access and VRAM/OAM reading
    ///
    /// # Returns
    /// * `true` if a complete frame was rendered, `false` otherwise
    ///
    /// # Timing Details
    ///
    /// - Scanlines 0-143: Visible rendering (456 cycles each)
    /// - Scanline 144: VBlank interrupt triggered
    /// - Scanlines 144-153: VBlank period (456 cycles each)
    /// - Scanline 154: Frame complete, new frame begins
    pub fn step(&mut self, cycles: u32, bus: &mut Bus) -> bool {
        let lcdc = bus.lcdc();
        let lcd_enabled = (lcdc & 0x80) != 0;

        if !lcd_enabled {
            // When LCD is disabled, reset PPU state
            self.cycle_counter = 0;
            self.scanline = 0;
            bus.set_ly(0);
            bus.set_stat_ppu_bits(0, false); // Mode 0
            return false;
        }

        self.cycle_counter = self.cycle_counter.wrapping_add(cycles);

        // Constants for Game Boy timing
        const SCANLINE_CYCLES: u32 = 456; // Cycles per scanline
        const TOTAL_SCANLINES: u8 = 154; // Total scanlines per frame

        let mut frame_completed = false;

        // Process complete scanlines
        while self.cycle_counter >= SCANLINE_CYCLES {
            self.cycle_counter -= SCANLINE_CYCLES;

            // Render current scanline before moving to the next
            if self.scanline < 144 {
                self.render_scanline(bus, self.scanline as usize);
            }

            self.scanline = self.scanline.wrapping_add(1);

            // Tick HDMA once per completed scanline.
            // On real GBC hardware, H-Blank DMA transfers 16 bytes per scanline
            // during the HBlank period — including VBlank scanlines (144-153).
            // Previously tick_hdma was only called on mode 0 transitions, which
            // never occur during VBlank (always mode 1), causing games that start
            // HDMA during VBlank to freeze while polling FF55.
            bus.tick_hdma();

            // VBlank starts at scanline 144
            if self.scanline == 144 {
                // Trigger VBlank interrupt (IF bit 0)
                bus.request_interrupt(0x01);
            }

            // Frame complete at scanline 154
            if self.scanline >= TOTAL_SCANLINES {
                self.scanline = 0;
                self.frame_counter = self.frame_counter.wrapping_add(1);

                frame_completed = true;

                // Debug tracing for frame completion
                if trace_enabled() {
                    let lcdc = bus.lcdc();
                    let palette = bus.read8(0xFF47);
                    let stat = bus.stat();
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

            // Update LY register with current scanline
            bus.set_ly(self.scanline);
        }

        // Determine current LCD mode
        let mode = if self.scanline >= 144 {
            1 // Mode 1: VBlank
        } else if self.cycle_counter < 80 {
            2 // Mode 2: OAM Search
        } else if self.cycle_counter < 252 {
            3 // Mode 3: Pixel Transfer
        } else {
            0 // Mode 0: HBlank
        };

        // Determine if STAT interrupt should be requested
        let stat = bus.stat();
        let old_mode = stat & 0x03;
        let mut request_stat_int = false;

        if mode != old_mode {
            // Trigger STAT interrupt on mode transition if enabled
            match mode {
                0 => {
                    if stat & 0x08 != 0 {
                        request_stat_int = true;
                    }
                } // Mode 0 HBlank
                1 => {
                    if stat & 0x10 != 0 {
                        request_stat_int = true;
                    }
                } // Mode 1 VBlank
                2 => {
                    if stat & 0x20 != 0 {
                        request_stat_int = true;
                    }
                } // Mode 2 OAM Search
                _ => {}
            }
        }

        // Compare LY and LYC
        let lyc = bus.lyc();
        let lyc_match = self.scanline == lyc;
        let old_lyc_match = stat & 0x04 != 0;

        if lyc_match && !old_lyc_match {
            if stat & 0x40 != 0 {
                request_stat_int = true;
            }
        }

        // Update STAT register bits in the bus
        bus.set_stat_ppu_bits(mode, lyc_match);

        if request_stat_int {
            bus.request_interrupt(0x02); // Set STAT interrupt flag (IF bit 1)
        }

        frame_completed
    }

    /// Returns a reference to the current framebuffer.
    ///
    /// The framebuffer contains the rendered image as 32-bit colors.
    pub fn framebuffer(&self) -> &[u32] {
        &self.framebuffer
    }

    /// Renders a complete frame by drawing background, window, and sprites.
    ///
    /// This function is called once per frame (every 70,224 cycles) and
    /// completely redraws the 160x144 pixel framebuffer.
    ///
    /// # Rendering Order
    fn render_scanline(&mut self, bus: &Bus, y: usize) {
        let lcdc = bus.read8(0xFF40);
        let is_cgb = bus.is_cgb;
        
        let bg_enabled = (lcdc & 0x01) != 0 || is_cgb;

        if !bg_enabled {
            let start_idx = y * SCREEN_WIDTH;
            let end_idx = start_idx + SCREEN_WIDTH;
            self.framebuffer[start_idx..end_idx].fill(0xFFFFFFFF);
            self.bg_pixel_info[start_idx..end_idx].fill(0);
            return;
        }

        let scroll_y = bus.read8(0xFF42) as usize;
        let scroll_x = bus.read8(0xFF43) as usize;
        let palette = bus.read8(0xFF47);

        let bg_map_base = if lcdc & 0x08 != 0 { 0x9C00 } else { 0x9800 };

        let wy = bus.read8(0xFF4A) as usize;
        let wx = bus.read8(0xFF4B) as usize;
        let win_enabled = (lcdc & 0x20) != 0;
        let win_map_base = if lcdc & 0x40 != 0 { 0x9C00 } else { 0x9800 };
        let win_x_start = (wx as i32) - 7;

        let tile_data_signed = lcdc & 0x10 == 0;

        for x in 0..SCREEN_WIDTH {
            let in_window = win_enabled && wy < SCREEN_HEIGHT && y >= wy && (x as i32) >= win_x_start;

            let (map_y, mut tile_line, map_x, mut tile_col, map_base) = if in_window {
                let window_x = (x as i32 - win_x_start) as usize;
                let window_y = y - wy;
                (
                    window_y / 8,
                    (window_y % 8) as u16,
                    window_x / 8,
                    (window_x % 8) as u16,
                    win_map_base,
                )
            } else {
                (
                    ((y + scroll_y) & 0xFF) / 8,
                    ((y + scroll_y) & 0x07) as u16,
                    ((x + scroll_x) & 0xFF) / 8,
                    ((x + scroll_x) & 0x07) as u16,
                    bg_map_base,
                )
            };

            let tile_index_addr = map_base + (map_y * 32 + map_x) as u16;
            let tile_offset = tile_index_addr - 0x8000;
            let tile_index = bus.read_vram_bank(0, tile_offset);

            let (palette_num, vram_bank, x_flip, y_flip, priority) = if bus.is_cgb {
                let attr = bus.read_vram_bank(1, tile_offset);
                (
                    attr & 0x07,
                    (attr >> 3) & 0x01,
                    (attr & 0x20) != 0,
                    (attr & 0x40) != 0,
                    (attr & 0x80) != 0,
                )
            } else {
                (0, 0, false, false, false)
            };

            if y_flip {
                tile_line = 7 - tile_line;
            }
            if x_flip {
                tile_col = 7 - tile_col;
            }

            let tile_addr = if tile_data_signed {
                let signed_index = tile_index as i8 as i16;
                0x9000u16.wrapping_add((signed_index * 16) as u16)
            } else {
                0x8000u16 + u16::from(tile_index) * 16
            };

            let line_offset = tile_addr.wrapping_add(tile_line * 2) - 0x8000;
            let b1 = bus.read_vram_bank(vram_bank, line_offset);
            let b2 = bus.read_vram_bank(vram_bank, line_offset + 1);

            let bit = 7 - tile_col;
            let color_index = ((b2 >> bit) & 1) << 1 | ((b1 >> bit) & 1);

            let pixel_idx = y * SCREEN_WIDTH + x;
            let final_color = if bus.is_cgb {
                let pal_offset = usize::from(palette_num) * 8 + usize::from(color_index) * 2;
                let low = bus.bg_palette_ram[pal_offset];
                let high = bus.bg_palette_ram[pal_offset + 1];
                let rgb555 = u16::from(high) << 8 | u16::from(low);
                
                let r = (rgb555 & 0x1F) as u32;
                let g = ((rgb555 >> 5) & 0x1F) as u32;
                let b = ((rgb555 >> 10) & 0x1F) as u32;
                
                let r8 = (r << 3) | (r >> 2);
                let g8 = (g << 3) | (g >> 2);
                let b8 = (b << 3) | (b >> 2);
                
                0xFF000000 | (r8 << 16) | (g8 << 8) | b8
            } else {
                let shade = (palette >> (color_index * 2)) & 0x03;
                match shade {
                    0 => 0xFFFFFFFF,
                    1 => 0xFFAAAAAA,
                    2 => 0xFF555555,
                    _ => 0xFF000000,
                }
            };

            self.bg_pixel_info[pixel_idx] = color_index | (if priority { 0x80 } else { 0x00 });
            self.framebuffer[pixel_idx] = final_color;
        }

        self.render_sprites_for_scanline(bus, lcdc, y);
    }

    fn render_sprites_for_scanline(&mut self, bus: &Bus, lcdc: u8, y: usize) {
        let sprites_enabled = lcdc & 0x02 != 0;
        if !sprites_enabled {
            return;
        }

        let sprite_height = if lcdc & 0x04 != 0 { 16 } else { 8 };
        let oam_base = 0xFE00u16;
        let palette0 = bus.read8(0xFF48);
        let palette1 = bus.read8(0xFF49);

        // Process all 40 sprites in OAM in reverse order so lower index has priority
        for sprite_idx in (0..40).rev() {
            let oam_offset = (sprite_idx * 4) as u16;

            let sprite_y = bus.read8(oam_base + oam_offset) as i16 - 16;
            
            // Check if this sprite intersects the current scanline
            if (y as i16) < sprite_y || (y as i16) >= sprite_y + sprite_height {
                continue;
            }

            let sprite_x = bus.read8(oam_base + oam_offset + 1) as i16 - 8;
            let tile_number = bus.read8(oam_base + oam_offset + 2);
            let attributes = bus.read8(oam_base + oam_offset + 3);

            let priority = attributes & 0x80 != 0;
            let y_flip = attributes & 0x40 != 0;
            let x_flip = attributes & 0x20 != 0;
            
            let (palette_num, vram_bank) = if bus.is_cgb {
                (attributes & 0x07, (attributes >> 3) & 0x01)
            } else {
                (if attributes & 0x10 != 0 { 1 } else { 0 }, 0)
            };

            let sy = y as i16 - sprite_y;
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

            let offset = tile_addr - 0x8000;
            let b1 = bus.read_vram_bank(vram_bank, offset);
            let b2 = bus.read_vram_bank(vram_bank, offset + 1);

            for sx in 0..8 {
                let screen_x = sprite_x + sx as i16;
                if screen_x < 0 || screen_x >= SCREEN_WIDTH as i16 {
                    continue;
                }

                let bit = if x_flip { sx } else { 7 - sx };
                let color_index = ((b2 >> bit) & 1) << 1 | ((b1 >> bit) & 1);

                if color_index == 0 {
                    continue;
                }

                let final_color = if bus.is_cgb {
                    let pal_offset = usize::from(palette_num) * 8 + usize::from(color_index) * 2;
                    let low = bus.sp_palette_ram[pal_offset];
                    let high = bus.sp_palette_ram[pal_offset + 1];
                    let rgb555 = u16::from(high) << 8 | u16::from(low);
                    
                    let r = (rgb555 & 0x1F) as u32;
                    let g = ((rgb555 >> 5) & 0x1F) as u32;
                    let b = ((rgb555 >> 10) & 0x1F) as u32;
                    
                    let r8 = (r << 3) | (r >> 2);
                    let g8 = (g << 3) | (g >> 2);
                    let b8 = (b << 3) | (b >> 2);
                    
                    0xFF000000 | (r8 << 16) | (g8 << 8) | b8
                } else {
                    let palette_reg = if palette_num == 1 { palette1 } else { palette0 };
                    let shade = (palette_reg >> (color_index * 2)) & 0x03;
                    match shade {
                        0 => 0xFFFFFFFF,
                        1 => 0xFFAAAAAA,
                        2 => 0xFF555555,
                        _ => 0xFF000000,
                    }
                };

                let pixel_idx = y * SCREEN_WIDTH + (screen_x as usize);

                let bg_info = self.bg_pixel_info[pixel_idx];
                let bg_color_idx = bg_info & 0x03;
                let bg_has_priority = (bg_info & 0x80) != 0;

                let sprite_behind_bg = if bus.is_cgb {
                    if (lcdc & 0x01) == 0 {
                        false
                    } else if bg_has_priority {
                        true
                    } else if priority {
                        bg_color_idx != 0
                    } else {
                        false
                    }
                } else {
                    priority && bg_color_idx != 0
                };

                if !sprite_behind_bg {
                    self.framebuffer[pixel_idx] = final_color;
                }
            }
        }
    }
}
