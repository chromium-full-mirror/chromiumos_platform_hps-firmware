MEMORY
{
  HDR   (rx) : ORIGIN = 0x08002000, LENGTH = 128
  FLASH (rx) : ORIGIN = 0x08002080, LENGTH = 8K - LENGTH(HDR)
  OTP    (r) : ORIGIN = 0x1FFF7000, LENGTH = 1K
  CFG   (rw) : ORIGIN = 0x1FFF7800, LENGTH = 128
  RAM  (xrw) : ORIGIN = 0x20000000, LENGTH = 8K
}

SECTIONS
{
    .image_hdr : {
        KEEP (*(.image_hdr))
    } > HDR
}
