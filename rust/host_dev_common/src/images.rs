// Copyright 2021 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use anyhow::anyhow;
use anyhow::bail;
use anyhow::Result;
use fpga_app::camera::END_IMAGE_MARKER;
use fpga_app::camera::IMAGE_HEIGHT;
use fpga_app::camera::IMAGE_WIDTH;
use fpga_app::camera::START_IMAGE_MARKER;
use image::GrayImage;
use image::ImageBuffer;
use image::RgbImage;
use std::io::Cursor;
use std::num::Wrapping;
use std::os::unix::prelude::OsStrExt;
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;
use std::time::Instant;

#[derive(Clone, Copy)]
pub enum ImageType {
    Grayscale,
    Color,
}

pub struct OutputDir {
    path: PathBuf,
    image_number: Option<u32>,
    image_type: ImageType,
    log_fn: Box<dyn FnMut(String) -> Result<()>>,
    raw_output: bool,
    latest_symlink: bool,
    wait_parent_dir_exists: bool,
    error_reported: bool,
    last_receive_timestamp: Instant,
    image_count: u64,
}

impl OutputDir {
    fn maybe_link_file(&self, original: &Path, link: &Path) -> Result<()> {
        if !self.latest_symlink {
            return Ok(());
        }
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
    fn write_png_from_greyscale_camera(&self, filename: &Path, raw_bytes: &[u8]) -> Result<()> {
        let image: GrayImage = ImageBuffer::from_fn(IMAGE_WIDTH, IMAGE_HEIGHT, |x, y| {
            image::Luma([raw_bytes[(y * IMAGE_WIDTH + x) as usize]])
        });
        image.save(&filename)?;
        self.maybe_link_file(
            Path::new(filename.file_name().unwrap()),
            &filename.with_file_name("latest.png"),
        )?;
        Ok(())
    }

    /// Takes color image data and writes it as both a color PNG and a grayscale PNG.
    fn write_pngs_from_color_camera(&self, filename: &Path, raw_bytes: &[u8]) -> Result<()> {
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
        self.maybe_link_file(
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
        self.maybe_link_file(
            &greyscale_filename,
            &filename.with_file_name("latest.grey.png"),
        )?;

        Ok(())
    }

    pub fn new(
        path: PathBuf,
        image_type: ImageType,
        log_fn: Box<dyn FnMut(String) -> Result<()>>,
    ) -> Result<Self> {
        Ok(Self {
            path,
            image_number: None,
            image_type,
            log_fn,
            raw_output: true,
            latest_symlink: true,
            wait_parent_dir_exists: false,
            error_reported: false,
            last_receive_timestamp: Instant::now(),
            image_count: 0,
        })
    }

    pub fn set_raw_output(&mut self, raw_output: bool) {
        self.raw_output = raw_output;
    }

    pub fn set_latest_symlink(&mut self, latest_symlink: bool) {
        self.latest_symlink = latest_symlink;
    }

    pub fn set_wait_parent_dir_exists(&mut self, wait_parent_dir_exists: bool) {
        self.wait_parent_dir_exists = wait_parent_dir_exists;
    }

    /// Returns the number of images we've received and written to the
    /// filesystem.
    pub fn image_count(&self) -> u64 {
        self.image_count
    }

    /// Returns how long it has been since we received an image (or started if
    /// we've never received an image).
    pub fn duration_without_images(&self) -> Duration {
        self.last_receive_timestamp.elapsed()
    }

    /// If `buffer` contains a complete image, write it to the output directory
    /// and remove it from `buffer`.
    pub fn process_images_from_buffer(&mut self, buffer: &mut Vec<u8>) -> Result<()> {
        if let Some(end) = index_of(buffer, &END_IMAGE_MARKER) {
            let mut write_result: Result<()> = Ok(());
            if let Some(start) = index_of(buffer, &START_IMAGE_MARKER) {
                let start = start + START_IMAGE_MARKER.len();
                let bytes = &mut buffer[start..end];
                // The HPS converts images to signed (i8). Convert them back to
                // unsigned.
                for b in bytes.iter_mut() {
                    *b = (Wrapping(*b) + Wrapping(128)).0;
                }
                if !self.wait_parent_dir_exists
                    || self
                        .path
                        .parent()
                        .map(|parent| parent.exists())
                        .unwrap_or(false)
                {
                    std::fs::create_dir_all(&self.path)
                        .map_err(|e| anyhow!("Error creating directory {:?}: {}", self.path, e))?;
                }
                self.last_receive_timestamp = Instant::now();
                write_result = self.write_image(bytes);
                if self.wait_parent_dir_exists {
                    // When in this mode, the output directory could be removed
                    // at any time, so just print the first such error, then
                    // continue.
                    if let Err(error) = write_result {
                        if !self.error_reported {
                            eprintln!("Error writing image: {:?}", error);
                            self.error_reported = true;
                        }
                    } else {
                        self.error_reported = false;
                    }
                    write_result = Ok(());
                }
            }
            // If we didn't have a START_IMAGE, then we have an incomplete
            // image, so we remove everything up to the end of the image
            // regardless.
            buffer.drain(..end + END_IMAGE_MARKER.len());
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
        let png_filename = self.next_output_filename()?;
        if self.raw_output {
            let raw_filename = png_filename.with_extension("raw");
            std::fs::write(&raw_filename, raw_bytes)?;
            (self.log_fn)(format!("Wrote {:?}", raw_filename))?;
        }
        self.write_png(&png_filename, raw_bytes)?;
        (self.log_fn)(format!("Wrote {:?}", png_filename))?;
        self.image_count += 1;
        Ok(())
    }

    fn write_png(&self, png_filename: &Path, raw_bytes: &[u8]) -> Result<(), anyhow::Error> {
        match self.image_type {
            ImageType::Grayscale => {
                self.write_png_from_greyscale_camera(png_filename, raw_bytes)?
            }
            ImageType::Color => self.write_pngs_from_color_camera(png_filename, raw_bytes)?,
        }
        Ok(())
    }

    fn next_output_filename(&mut self) -> Result<PathBuf> {
        if self.image_number.is_none() {
            self.image_number = Some(starting_sequence_number(&self.path)?);
        }
        let image_number = self.image_number.as_mut().unwrap();
        let filename = self.path.join(format!("{:08}.png", *image_number));
        *image_number += 1;
        Ok(filename)
    }
}

/// If `dir_path` is a directory containing files with names like 00123.xyz,
/// then returns the next number that is larger than all the existing numbered
/// files. Gaps will be ignored. So if the directory contained 01.xyz, 05.xyz
/// and 06.xyz, then the return value would be 7, not 2.
fn starting_sequence_number(dir_path: &Path) -> Result<u32> {
    let mut next = 0;
    for entry in dir_path.read_dir()?.flatten() {
        if let Some(stem) = Path::new(&entry.file_name()).file_stem() {
            if let Ok(stem_utf8) = core::str::from_utf8(stem.as_bytes()) {
                if let Ok(number) = stem_utf8.parse::<u32>() {
                    next = next.max(number + 1);
                }
            }
        }
    }
    Ok(next)
}

fn index_of(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    for (index, values) in haystack.windows(needle.len()).enumerate() {
        if values == needle {
            return Some(index);
        }
    }
    None
}

pub fn image_type_from_str(value: &str) -> Result<ImageType, String> {
    Ok(if value.eq_ignore_ascii_case("color") {
        ImageType::Color
    } else if value.eq_ignore_ascii_case("grayscale") {
        ImageType::Grayscale
    } else {
        return Err("Color type needs to be one of 'color' or 'grayscale'".to_owned());
    })
}

#[cfg(test)]
mod tests {
    use anyhow::Result;

    // Note, this test is currently disabled when running under MIRI since it
    // does filesystem access. We don't have any unsafe code in the function
    // being tested, so this isn't a big loss.
    #[test]
    fn test_starting_sequence_number_nomiri() -> Result<()> {
        let dir = tempfile::TempDir::new()?;
        std::fs::write(dir.path().join("0003.png"), "")?;
        std::fs::write(dir.path().join("0006.png"), "")?;
        assert_eq!(super::starting_sequence_number(dir.path())?, 7);
        Ok(())
    }
}
