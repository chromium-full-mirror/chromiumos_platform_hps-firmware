MEMORY
{
  HDR   (rx) : ORIGIN = 0x08004000, LENGTH = 128
  FLASH (rx) : ORIGIN = 0x08004080, LENGTH = 112K - LENGTH(HDR)
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
