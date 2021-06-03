// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use crate::ChannelHandler;
use anyhow::anyhow;
use anyhow::bail;
use anyhow::Result;
use image::GrayImage;
use image::ImageBuffer;
use image::RgbImage;
use std::io::Cursor;
use std::num::Wrapping;
use std::path::Path;
use std::path::PathBuf;

const IMAGE_WIDTH: u32 = 324;
const IMAGE_HEIGHT: u32 = 244;

// These constants are randomly generated to reduce the chances of them
// appearing by chance in actual image data. They are duplicated in the C code
// that sends the images.
const START_IMAGE: [u8; 8] = [13, 174, 212, 250, 191, 30, 138, 125];
const END_IMAGE: [u8; 8] = [227, 114, 212, 105, 101, 111, 21, 193];

/// Receives images and writes them out to disk in RAW format and PNG.
pub(crate) struct ImageChannel {
    received: Vec<u8>,
    output_dir: OutputDir,
}

#[derive(Clone, Copy)]
pub(crate) enum ImageType {
    Grayscale,
    Color,
}

struct OutputDir {
    path: PathBuf,
    image_number: u32,
    image_type: ImageType,
}

impl ImageChannel {
    pub(crate) fn new(out_dir: &Path, image_type: ImageType) -> Result<Self> {
        Ok(Self {
            received: Vec::new(),
            output_dir: OutputDir::new(out_dir.to_owned(), image_type)?,
        })
    }
}

impl ChannelHandler for ImageChannel {
    fn handle(&mut self, bytes: &[u8]) -> Result<()> {
        self.received.extend_from_slice(bytes);
        if let Some(end) = index_of(&self.received, &END_IMAGE) {
            let mut write_result: Result<()> = Ok(());
            if let Some(start) = index_of(&self.received, &START_IMAGE) {
                let start = start + START_IMAGE.len();
                let bytes = &mut self.received[start..end];
                // The HPS converts images to signed (i8). Convert them back to
                // unsigned.
                for b in bytes.iter_mut() {
                    *b = (Wrapping(*b) + Wrapping(128)).0;
                }
                write_result = self.output_dir.write_image(bytes);
            }
            // If we didn't have a START_IMAGE, then we have an incomplete
            // image, so we remove everything up to the end of the image
            // regardless.
            self.received.drain(..end + END_IMAGE.len());
            // We check for errors writing images after we've drained the data,
            // otherwise if there's a problem with the image data, we'd keep it
            // in our buffer and try to process it again next time.
            write_result?;
        }
        Ok(())
    }
}

fn write_as_greyscale_png(filename: &Path, raw_bytes: &[u8]) -> Result<()> {
    let image: GrayImage = ImageBuffer::from_fn(IMAGE_WIDTH, IMAGE_HEIGHT, |x, y| {
        image::Luma([raw_bytes[(y * IMAGE_WIDTH + x) as usize]])
    });
    Ok(image.save(filename)?)
}

fn write_as_color_png(filename: &Path, raw_bytes: &[u8]) -> Result<()> {
    let mut buf = vec![0; (3 * raw_bytes.len()) as usize];
    let mut dst = bayer::RasterMut::new(
        IMAGE_WIDTH as usize,
        IMAGE_HEIGHT as usize,
        bayer::RasterDepth::Depth8,
        &mut buf,
    );
    bayer::run_demosaic(
        &mut Cursor::new(&raw_bytes[..]),
        bayer::BayerDepth::Depth8,
        bayer::CFA::GBRG,
        bayer::Demosaic::Cubic,
        &mut dst,
    )?;

    let image: RgbImage = ImageBuffer::from_fn(IMAGE_WIDTH, IMAGE_HEIGHT, |x, y| {
        let r = buf[(y * IMAGE_WIDTH * 3 + x * 3) as usize];
        let g = buf[(y * IMAGE_WIDTH * 3 + x * 3 + 1) as usize];
        let b = buf[(y * IMAGE_WIDTH * 3 + x * 3 + 2) as usize];
        image::Rgb([r, g, b])
    });
    Ok(image.save(filename)?)
}

impl OutputDir {
    fn new(path: PathBuf, image_type: ImageType) -> Result<Self> {
        std::fs::create_dir_all(&path)
            .map_err(|e| anyhow!("Error creating directory {:?}: {}", path, e))?;
        Ok(Self {
            path,
            image_number: 0,
            image_type,
        })
    }

    fn write_image(&mut self, raw_bytes: &[u8]) -> Result<()> {
        if raw_bytes.len() != (IMAGE_WIDTH * IMAGE_HEIGHT) as usize {
            bail!(
                "Image bytes is the wrong size, expected {} bytes, but got {}",
                IMAGE_WIDTH * IMAGE_HEIGHT,
                raw_bytes.len()
            );
        }
        let raw_filename = self.next_output_filename();
        std::fs::write(&raw_filename, raw_bytes)?;
        self.write_png(&raw_filename.with_extension("png"), raw_bytes)?;
        self.write_png(&self.path.join("latest.png"), raw_bytes)?;
        println!("Wrote {:?}", raw_filename);
        Ok(())
    }

    fn write_png(&self, png_filename: &PathBuf, raw_bytes: &[u8]) -> Result<(), anyhow::Error> {
        Ok(match self.image_type {
            ImageType::Grayscale => write_as_greyscale_png(&png_filename, raw_bytes)?,
            ImageType::Color => write_as_color_png(&png_filename, raw_bytes)?,
        })
    }

    fn next_output_filename(&mut self) -> PathBuf {
        loop {
            let filename = self.path.join(format!("{:04}.raw", self.image_number));
            if !filename.exists() {
                return filename;
            }
            self.image_number += 1;
        }
    }
}

fn index_of(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    for (index, values) in haystack.windows(needle.len()).enumerate() {
        if values == needle {
            return Some(index);
        }
    }
    None
}

pub(crate) fn image_type_from_str(value: &str) -> Result<ImageType, String> {
    Ok(if value.eq_ignore_ascii_case("color") {
        ImageType::Color
    } else if value.eq_ignore_ascii_case("grayscale") {
        ImageType::Grayscale
    } else {
        return Err("Color type needs to be one of 'color' or 'grayscale'".to_owned());
    })
}
