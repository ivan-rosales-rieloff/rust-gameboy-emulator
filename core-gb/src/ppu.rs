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
    /// The final rendered image (160x144 pixels, 1 byte per pixel)
    /// Each pixel contains a palette index (0-3) representing shade
    #[serde(with = "serde_array")]
    pub framebuffer: [u8; SCREEN_WIDTH * SCREEN_HEIGHT],

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
            framebuffer: [0; SCREEN_WIDTH * SCREEN_HEIGHT],
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
        self.cycle_counter = self.cycle_counter.wrapping_add(cycles);

        // Constants for Game Boy timing
        const SCANLINE_CYCLES: u32 = 456; // Cycles per scanline
        const TOTAL_SCANLINES: u8 = 154; // Total scanlines per frame

        let mut frame_completed = false;

        // Process complete scanlines
        while self.cycle_counter >= SCANLINE_CYCLES {
            self.cycle_counter -= SCANLINE_CYCLES;
            self.scanline = self.scanline.wrapping_add(1);

            // VBlank starts at scanline 144
            if self.scanline == 144 {
                // Trigger VBlank interrupt (IF bit 0)
                bus.request_interrupt(0x01);
            }

            // Frame complete at scanline 154
            if self.scanline >= TOTAL_SCANLINES {
                self.scanline = 0;
                self.frame_counter = self.frame_counter.wrapping_add(1);

                // Render the complete frame
                self.render_frame(bus);
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
    /// The framebuffer contains the rendered image as palette indices (0-3).
    /// To get actual colors, these indices must be mapped through the palette.
    pub fn framebuffer(&self) -> &[u8; SCREEN_WIDTH * SCREEN_HEIGHT] {
        &self.framebuffer
    }

    /// Renders a complete frame by drawing background, window, and sprites.
    ///
    /// This function is called once per frame (every 70,224 cycles) and
    /// completely redraws the 160x144 pixel framebuffer.
    ///
    /// # Rendering Order
    /// 1. Background tiles (if enabled)
    /// 2. Window tiles (if enabled)
    /// 3. Sprites (if enabled, with priority handling)
    fn render_frame(&mut self, bus: &Bus) {
        // Read LCD control register to determine what to render
        let lcdc = bus.read8(0xFF40);
        let bg_enabled = lcdc & 0x01 != 0; // Bit 0: BG enable

        // If background is disabled, clear screen to color 0
        if !bg_enabled {
            self.framebuffer.fill(0);
            return;
        }

        // Read rendering parameters from I/O registers
        let scroll_y = bus.read8(0xFF42) as usize; // Background scroll Y
        let scroll_x = bus.read8(0xFF43) as usize; // Background scroll X
        let palette = bus.read8(0xFF47); // Background palette

        // Determine tile map base address (0x9800 or 0x9C00)
        let bg_map_base = if lcdc & 0x08 != 0 { 0x9C00 } else { 0x9800 };

        // Determine Window parameters
        let wy = bus.read8(0xFF4A) as usize; // Window Y
        let wx = bus.read8(0xFF4B) as usize; // Window X
        let win_enabled = (lcdc & 0x20) != 0; // Bit 5: Window enable
        let win_map_base = if lcdc & 0x40 != 0 { 0x9C00 } else { 0x9800 }; // Bit 6: Window map base
        let win_x_start = (wx as i32) - 7;

        // Determine tile data addressing mode
        let tile_data_signed = lcdc & 0x10 == 0;

        // Render background and window pixel by pixel
        for y in 0..SCREEN_HEIGHT {
            for x in 0..SCREEN_WIDTH {
                // Determine if we should render the window or background pixel
                let in_window =
                    win_enabled && wy < SCREEN_HEIGHT && y >= wy && (x as i32) >= win_x_start;

                let (map_y, tile_line, map_x, tile_col, map_base) = if in_window {
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

                // Get tile index from the appropriate map
                let tile_index_addr = map_base + (map_y * 32 + map_x) as u16;
                let tile_index = bus.read8(tile_index_addr);

                // Calculate tile data address based on addressing mode
                let tile_addr = if tile_data_signed {
                    // Signed mode: tile_index is signed offset from 0x9000
                    let signed_index = tile_index as i8 as i16;
                    0x9000u16.wrapping_add((signed_index * 16) as u16)
                } else {
                    // Unsigned mode: tile_index is direct offset from 0x8000
                    0x8000u16 + u16::from(tile_index) * 16
                };

                // Get the two bytes for this pixel row (8 pixels, 2 bits each)
                let line_addr = tile_addr.wrapping_add(tile_line * 2);
                let b1 = bus.read8(line_addr); // Low bit plane
                let b2 = bus.read8(line_addr.wrapping_add(1)); // High bit plane

                // Extract color index for this pixel (2 bits)
                let bit = 7 - tile_col; // MSB first (bit 7 = leftmost pixel)
                let color_index = ((b2 >> bit) & 1) << 1 | ((b1 >> bit) & 1);

                // Apply palette to get final shade (0-3)
                let shade = (palette >> (color_index * 2)) & 0x03;

                // Store pixel in framebuffer
                self.framebuffer[y * SCREEN_WIDTH + x] = shade;
            }
        }

        // Render sprites on top of background
        self.render_sprites(bus, lcdc);
    }

    /// Renders all sprites (OAM objects) for the current frame.
    ///
    /// Sprites are rendered after the background and can appear in front of
    /// or behind background pixels based on their priority attribute.
    ///
    /// # Sprite Processing
    /// - Up to 40 sprites total, max 10 per scanline
    /// - 8x8 or 8x16 pixels depending on LCDC bit 2
    /// - Color 0 is transparent
    /// - Priority determines layering with background
    fn render_sprites(&mut self, bus: &Bus, lcdc: u8) {
        let sprites_enabled = lcdc & 0x02 != 0; // Bit 1: Sprite enable
        if !sprites_enabled {
            return;
        }

        // Determine sprite height from LCDC bit 2
        let sprite_height = if lcdc & 0x04 != 0 { 16 } else { 8 };

        // OAM base address and sprite palettes
        let oam_base = 0xFE00u16;
        let palette0 = bus.read8(0xFF48); // Object palette 0
        let palette1 = bus.read8(0xFF49); // Object palette 1

        // Process all 40 sprites in OAM (Game Boy doesn't sort by priority)
        for sprite_idx in 0..40 {
            let oam_offset = (sprite_idx * 4) as u16;

            // Read sprite attributes from OAM
            let sprite_y = bus.read8(oam_base + oam_offset) as i16 - 16; // Y position (top)
            let sprite_x = bus.read8(oam_base + oam_offset + 1) as i16 - 8; // X position (left)
            let tile_number = bus.read8(oam_base + oam_offset + 2); // Tile index
            let attributes = bus.read8(oam_base + oam_offset + 3); // Attributes

            // Parse sprite attributes
            let priority = attributes & 0x80 != 0; // Bit 7: Priority (0=above BG, 1=behind BG)
            let y_flip = attributes & 0x40 != 0; // Bit 6: Vertical flip
            let x_flip = attributes & 0x20 != 0; // Bit 5: Horizontal flip
            let palette_num = attributes & 0x10 != 0; // Bit 4: Palette select (0=OBP0, 1=OBP1)
            let palette = if palette_num { palette1 } else { palette0 };

            // Render each pixel row of the sprite
            for sy in 0..sprite_height {
                // Calculate screen Y position
                let screen_y = sprite_y + sy as i16;
                if screen_y < 0 || screen_y >= SCREEN_HEIGHT as i16 {
                    continue; // Sprite row is off-screen
                }

                // Handle vertical flipping
                let tile_y = if y_flip {
                    (sprite_height - 1 - sy) as u16
                } else {
                    sy as u16
                };

                // Calculate tile data address
                let tile_addr = if sprite_height == 16 {
                    // 8x16 sprites use two tiles (even/odd based on bit 0)
                    0x8000u16 + u16::from(tile_number & 0xFE) * 16 + tile_y * 2
                } else {
                    // 8x8 sprites use single tile
                    0x8000u16 + u16::from(tile_number) * 16 + tile_y * 2
                };

                // Read tile data for this row
                let b1 = bus.read8(tile_addr);
                let b2 = bus.read8(tile_addr + 1);

                // Render each pixel in the row
                for sx in 0..8 {
                    // Calculate screen X position
                    let screen_x = sprite_x + sx as i16;
                    if screen_x < 0 || screen_x >= SCREEN_WIDTH as i16 {
                        continue; // Sprite pixel is off-screen
                    }

                    // Handle horizontal flipping
                    let bit = if x_flip { sx } else { 7 - sx };

                    // Extract color index (2 bits per pixel)
                    let color_index = ((b2 >> bit) & 1) << 1 | ((b1 >> bit) & 1);

                    // Color 0 is transparent for sprites
                    if color_index == 0 {
                        continue;
                    }

                    // Apply palette to get final shade
                    let shade = (palette >> (color_index * 2)) & 0x03;
                    let pixel_idx = (screen_y as usize) * SCREEN_WIDTH + (screen_x as usize);

                    // Handle sprite priority
                    if priority {
                        // Behind background: only draw if background pixel is color 0
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
