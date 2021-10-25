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
use std::sync::mpsc;

const IMAGE_WIDTH: u32 = 320;
const IMAGE_HEIGHT: u32 = 240;

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
    log_fn: Box<dyn FnMut(String) -> Result<()>>,
}

impl ImageChannel {
    pub(crate) fn new(
        out_dir: &Path,
        image_type: ImageType,
        line_sink: mpsc::Sender<String>,
    ) -> Result<Self> {
        Ok(Self {
            received: Vec::new(),
            output_dir: OutputDir::new(
                out_dir.to_owned(),
                image_type,
                Box::new(move |log_line| line_sink.send(log_line).map_err(|e| anyhow!("{}", e))),
            )?,
        })
    }
}

impl ChannelHandler for ImageChannel {
    fn handle(&mut self, bytes: &[u8]) -> Result<()> {
        self.received.extend_from_slice(bytes);
        self.output_dir.process_images_from_buffer(&mut self.received)
    }
}

fn link_file(original: &Path, link: &Path) -> Result<()> {
    // Try to delete the link if it already exists, but ignore errors.
    let _ = std::fs::remove_file(link);
    std::os::unix::fs::symlink(original, link).map_err(|e| {
        anyhow!(
            "Error creating symlink from {:?} to {:?}: {:?}",
            original,
            link,
            e
        )
    })
}

/// Takes grayscale image data and writes it as a grayscale PNG.
fn write_png_from_greyscale_camera(filename: &Path, raw_bytes: &[u8]) -> Result<()> {
    let image: GrayImage = ImageBuffer::from_fn(IMAGE_WIDTH, IMAGE_HEIGHT, |x, y| {
        image::Luma([raw_bytes[(y * IMAGE_WIDTH + x) as usize]])
    });
    image.save(&filename)?;
    link_file(filename, &filename.with_file_name("latest.png"))?;
    Ok(())
}

/// Takes color image data and writes it as both a color PNG and a grayscale PNG.
fn write_pngs_from_color_camera(filename: &Path, raw_bytes: &[u8]) -> Result<()> {
    let mut buf = vec![0; (3 * raw_bytes.len()) as usize];
    let mut dst = bayer::RasterMut::new(
        IMAGE_WIDTH as usize,
        IMAGE_HEIGHT as usize,
        bayer::RasterDepth::Depth8,
        &mut buf,
    );
    bayer::run_demosaic(
        &mut Cursor::new(raw_bytes),
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
    let color_filename = filename.with_extension("color.png");
    image.save(&color_filename)?;
    link_file(
        &color_filename,
        &filename.with_file_name("latest.color.png"),
    )?;

    let image: GrayImage = ImageBuffer::from_fn(IMAGE_WIDTH, IMAGE_HEIGHT, |x, y| {
        let r = buf[(y * IMAGE_WIDTH * 3 + x * 3) as usize] as u16;
        let g = buf[(y * IMAGE_WIDTH * 3 + x * 3 + 1) as usize] as u16;
        let b = buf[(y * IMAGE_WIDTH * 3 + x * 3 + 2) as usize] as u16;
        image::Luma([((r + g + b) / 3) as u8])
    });
    let greyscale_filename = filename.with_extension("grey.png");
    image.save(&greyscale_filename)?;
    link_file(
        &greyscale_filename,
        &filename.with_file_name("latest.grey.png"),
    )?;

    Ok(())
}

impl OutputDir {
    fn new(
        path: PathBuf,
        image_type: ImageType,
        log_fn: Box<dyn FnMut(String) -> Result<()>>,
    ) -> Result<Self> {
        std::fs::create_dir_all(&path)
            .map_err(|e| anyhow!("Error creating directory {:?}: {}", path, e))?;
        Ok(Self {
            path,
            image_number: 0,
            image_type,
            log_fn,
        })
    }

    /// If `buffer` contains a complete image, write it to the output directory
    /// and remove it from `buffer`.
    pub fn process_images_from_buffer(&mut self, buffer: &mut Vec<u8>) -> Result<()> {
        if let Some(end) = index_of(buffer, &END_IMAGE) {
            let mut write_result: Result<()> = Ok(());
            if let Some(start) = index_of(buffer, &START_IMAGE) {
                let start = start + START_IMAGE.len();
                let bytes = &mut buffer[start..end];
                // The HPS converts images to signed (i8). Convert them back to
                // unsigned.
                for b in bytes.iter_mut() {
                    *b = (Wrapping(*b) + Wrapping(128)).0;
                }
                write_result = self.write_image(bytes);
            }
            // If we didn't have a START_IMAGE, then we have an incomplete
            // image, so we remove everything up to the end of the image
            // regardless.
            buffer.drain(..end + END_IMAGE.len());
            // We check for errors writing images after we've drained the data,
            // otherwise if there's a problem with the image data, we'd keep it
            // in our buffer and try to process it again next time.
            write_result?;
        }
        Ok(())
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
        (self.log_fn)(format!("Wrote {:?}", raw_filename))?;
        Ok(())
    }

    fn write_png(&self, png_filename: &Path, raw_bytes: &[u8]) -> Result<(), anyhow::Error> {
        match self.image_type {
            ImageType::Grayscale => write_png_from_greyscale_camera(png_filename, raw_bytes)?,
            ImageType::Color => write_pngs_from_color_camera(png_filename, raw_bytes)?,
        }
        Ok(())
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
