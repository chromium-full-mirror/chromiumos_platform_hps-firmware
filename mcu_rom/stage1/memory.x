MEMORY
{
  # The header doesn't need 256 bytes, however the vector table is located at
  # the start of FLASH and the vector table must be 256 byte aligned, so we
  # give the extra space to the header.
  HDR   (rx) : ORIGIN = 0x08010000, LENGTH = 256
  FLASH (rx) : ORIGIN = 0x08010100, LENGTH = 64K - LENGTH(HDR)
  OTP    (r) : ORIGIN = 0x1FFF7000, LENGTH = 1K
  CFG   (rw) : ORIGIN = 0x1FFF7800, LENGTH = 128
  RAM  (xrw) : ORIGIN = 0x20000000, LENGTH = 36K
}

SECTIONS
{
    .image_hdr : {
        KEEP (*(.image_hdr))
    } > HDR
}
