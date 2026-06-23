use core_common::LinkEndpoint;
use serde::{Deserialize, Serialize};
use std::io;
use std::sync::mpsc::Sender;

/// Represents an image produced by the Game Boy Printer.
/// The image is 160 pixels wide. The height varies based on the number of printed tiles.
/// The data is an array of grayscale values (0-255).
#[derive(Clone)]
pub struct PrinterImage {
    pub width: usize,
    pub height: usize,
    pub data: Vec<u8>,
}

/// Emulates a Game Boy Printer peripheral connected via the Serial Link.
pub struct GbPrinter {
    status: u8,
    state: u32,
    data: Vec<u8>,
    packet: Vec<u8>,
    count: usize,
    datacount: usize,
    datasize: usize,
    result: u8,
    // Callback or channel to send completed images
    image_tx: Option<Sender<PrinterImage>>,
}

impl std::fmt::Debug for GbPrinter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "GbPrinter {{ state: {}, count: {} }}", self.state, self.count)
    }
}

impl LinkEndpoint for GbPrinter {
    fn transfer_byte(&mut self, byte: u8) -> io::Result<u8> {
        Ok(self.send(byte))
    }
}

impl GbPrinter {
    pub fn new(image_tx: Option<Sender<PrinterImage>>) -> Self {
        Self {
            status: 0,
            state: 0,
            data: vec![0; 0x280 * 9],
            packet: vec![0; 0x400],
            count: 0,
            datacount: 0,
            datasize: 0,
            result: 0,
            image_tx,
        }
    }

    fn check_crc(&self) -> bool {
        let mut crc = 0u16;
        for i in 2..(6 + self.datasize) {
            crc = crc.wrapping_add(self.packet[i] as u16);
        }

        let msgcrc = (self.packet[6 + self.datasize] as u16)
            .wrapping_add((self.packet[7 + self.datasize] as u16) << 8);

        crc == msgcrc
    }

    fn reset(&mut self) {
        self.state = 0;
        self.datasize = 0;
        self.datacount = 0;
        self.count = 0;
        self.status = 0;
        self.result = 0;
    }

    fn output_image(&mut self) {
        let image_height = self.datacount / 40;
        if image_height == 0 {
            return;
        }

        if let Some(tx) = &self.image_tx {
            let mut img_data = Vec::with_capacity(160 * image_height);
            let palbyte = self.packet[8];
            // Decode palette
            let palette = [
                3 - ((palbyte >> 0) & 3),
                3 - ((palbyte >> 2) & 3),
                3 - ((palbyte >> 4) & 3),
                3 - ((palbyte >> 6) & 3),
            ];

            // Map palette indices (0-3) to grayscale values (0-255)
            // 0 -> White (255), 3 -> Black (0)
            let gray_shades = [255, 170, 85, 0];

            for y in 0..image_height {
                for x in 0..160 {
                    let tilenumber = ((y >> 3) * 20) + (x >> 3);
                    let tileoffset = tilenumber * 16 + (y & 7) * 2;
                    let bx = 7 - (x & 7);

                    let colourindex = ((self.data[tileoffset] >> bx) & 1)
                        | (((self.data[tileoffset + 1] >> bx) << 1) & 2);

                    let shade_index = palette[colourindex as usize] as usize;
                    img_data.push(gray_shades[shade_index]);
                }
            }

            let _ = tx.send(PrinterImage {
                width: 160,
                height: image_height,
                data: img_data,
            });
        }
    }

    fn receive(&mut self) {
        if self.packet[3] != 0 {
            // Compressed data
            let mut dataidx = 6;
            let mut destidx = self.datacount;

            while dataidx - 6 < self.datasize {
                let control = self.packet[dataidx];
                dataidx += 1;

                if control & 0x80 != 0 {
                    let curlen = ((control & 0x7F) + 2) as usize;
                    for _ in 0..curlen {
                        if destidx < self.data.len() {
                            self.data[destidx] = self.packet[dataidx];
                        }
                        destidx += 1;
                    }
                    dataidx += 1;
                } else {
                    let curlen = (control + 1) as usize;
                    for i in 0..curlen {
                        if destidx + i < self.data.len() && dataidx + i < self.packet.len() {
                            self.data[destidx + i] = self.packet[dataidx + i];
                        }
                    }
                    destidx += curlen;
                    dataidx += curlen;
                }
            }

            self.datacount = destidx;
        } else {
            // Uncompressed data
            for i in 0..self.datasize {
                if self.datacount + i < self.data.len() && 6 + i < self.packet.len() {
                    self.data[self.datacount + i] = self.packet[6 + i];
                }
            }
            self.datacount += self.datasize;
        }
    }

    fn command(&mut self) {
        match self.packet[2] {
            0x01 => {
                // Init
                self.datacount = 0;
                self.status = 0;
            }
            0x02 => {
                // Print
                self.output_image();
            }
            0x04 => {
                // Data
                self.receive();
            }
            _ => (),
        }
    }

    pub fn send(&mut self, v: u8) -> u8 {
        if self.count < self.packet.len() {
            self.packet[self.count] = v;
        }
        self.count += 1;

        match self.state {
            0 => {
                if v == 0x88 {
                    self.state = 1;
                } else {
                    self.reset();
                }
            }
            1 => {
                if v == 0x33 {
                    self.state = 2;
                } else {
                    self.reset();
                }
            }
            2 => {
                if self.count == 6 {
                    self.datasize = self.packet[4] as usize + ((self.packet[5] as usize) << 8);
                    if self.datasize > 0 {
                        self.state = 3;
                    } else {
                        self.state = 4;
                    }
                }
            }
            3 => {
                if self.count == self.datasize + 6 {
                    self.state = 4;
                }
            }
            4 => {
                self.state = 5;
            }
            5 => {
                if self.check_crc() {
                    self.command();
                }
                self.state = 6;
            }
            6 => {
                self.result = 0x81;
                self.state = 7;
            }
            7 => {
                self.result = self.status;
                self.state = 0;
                self.count = 0;
            }
            _ => self.reset(),
        }
        self.result
    }
}
