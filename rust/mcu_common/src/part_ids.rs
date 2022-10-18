// Copyright 2022 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use crate::Buffer;
use crate::Error;
use core::fmt::Display;

/// Part IDs register. For all IDs, a value of 0 indicates unknown.
#[derive(Debug, Default, PartialEq, Eq, Clone, Copy)]
pub struct PartIds {
    /// MCU device and revision IDs. Bits [11:0] are the device ID and should
    /// read 0x460. See RM04444 section 40.10.1 (DBG_IDCODE) for more detail.
    pub mcu_id: u32,
    /// Reserved for the FPGA's IDCODE if we find a way to read it without JTAG
    /// access (which we don't have).
    pub fpga_id: u32,
    /// The camera part ID. Should be 0x01B0.
    pub camera_id: u32,
    /// The JEDEC manufacturer code for the SPI flash.
    pub spi_flash_manufacturer_id: u32,
    /// The JEDEC device ID for the SPI flash.
    pub spi_flash_device_id: u32,
}

impl PartIds {
    pub fn write_to_buffer(&self, buffer: &mut Buffer) -> Result<(), Error> {
        buffer.push_u32_be(self.mcu_id)?;
        buffer.push_u32_be(self.fpga_id)?;
        buffer.push_u32_be(self.camera_id)?;
        buffer.push_u32_be(self.spi_flash_manufacturer_id)?;
        buffer.push_u32_be(self.spi_flash_device_id)?;
        Ok(())
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<PartIds, Error> {
        let mut input = bytes
            .chunks_exact(4)
            .map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]));
        Ok(PartIds {
            mcu_id: input.next().ok_or(Error::BufferOverrun)?,
            fpga_id: input.next().ok_or(Error::BufferOverrun)?,
            camera_id: input.next().ok_or(Error::BufferOverrun)?,
            spi_flash_manufacturer_id: input.next().ok_or(Error::BufferOverrun)?,
            spi_flash_device_id: input.next().ok_or(Error::BufferOverrun)?,
        })
    }

    /// Set the SPI flash device ID from bytes. SPI flash device IDs can have
    /// different lengths up to 4 bytes.
    pub fn set_spi_flash_device_id(&mut self, id: &[u8]) {
        let mut bytes = [0; 4];
        let len = id.len().min(bytes.len());
        bytes[4 - len..].copy_from_slice(&id[..len]);
        self.spi_flash_device_id = u32::from_be_bytes(bytes);
    }
}

impl Display for PartIds {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        writeln!(f, "MCU ID: 0x{:x}", self.mcu_id)?;
        writeln!(f, "Camera ID: 0x{:x}", self.camera_id)?;
        writeln!(
            f,
            "SPI flash: 0x{:x}:0x{:x}",
            self.spi_flash_manufacturer_id, self.spi_flash_device_id
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::MemBlock;

    fn as_bytes(part_ids: &PartIds) -> Vec<u8> {
        let mut buffer = Buffer::new(MemBlock::with_capacity(core::mem::size_of::<PartIds>()));
        part_ids.write_to_buffer(&mut buffer).unwrap();
        buffer.to_owned()
    }

    #[test]
    fn test_part_ids_write_to_buffer() {
        let part_ids = PartIds {
            mcu_id: 0x1234_5678,
            fpga_id: 0x9876_5432,
            camera_id: 0xabcd,
            spi_flash_manufacturer_id: 0x01,
            spi_flash_device_id: 0x01020304,
        };
        let bytes = as_bytes(&part_ids);
        assert_eq!(
            &bytes,
            &[
                0x12, 0x34, 0x56, 0x78, // MCU ID
                0x98, 0x76, 0x54, 0x32, // FPGA ID
                0x00, 0x00, 0xab, 0xcd, // Camera ID
                0x00, 0x00, 0x00, 0x01, // SPI flash manufacturer
                0x01, 0x02, 0x03, 0x04, // SPI flash device ID
            ]
        );
        assert_eq!(PartIds::from_bytes(&bytes), Ok(part_ids));
    }

    #[test]
    fn test_part_ids_from_bytes() {
        // Bytes shorter than PartIds:
        assert_eq!(PartIds::from_bytes(&[]), Err(Error::BufferOverrun));
        assert_eq!(PartIds::from_bytes(&[1, 2]), Err(Error::BufferOverrun));
        // Bytes longer than PartIds:
        let mut bytes = as_bytes(&PartIds::default());
        bytes.push(1);
        assert_eq!(PartIds::from_bytes(&bytes), Ok(PartIds::default()));
    }

    #[test]
    fn test_set_spi_flash_device_id() {
        let mut ids = PartIds::default();
        ids.set_spi_flash_device_id(&[1]);
        assert_eq!({ ids.spi_flash_device_id }, 1);
        // No current device IDs are longer than 4 bytes, but just in case, make
        // sure we handle it by discarding the extra bytes.
        ids.set_spi_flash_device_id(&[1, 2, 3, 4, 5, 6]);
        assert_eq!({ ids.spi_flash_device_id }, 0x01020304);
        ids.set_spi_flash_device_id(&[9, 8]);
        assert_eq!({ ids.spi_flash_device_id }, 0x0908);
    }
}
