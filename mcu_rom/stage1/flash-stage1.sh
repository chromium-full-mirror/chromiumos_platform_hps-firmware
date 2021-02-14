#!/bin/sh

arm-none-eabi-objcopy -O binary target/thumbv6m-none-eabi/release/stage1 target/thumbv6m-none-eabi/release/stage1.bin
cp -v target/thumbv6m-none-eabi/release/stage1.bin target/thumbv6m-none-eabi/release/stage1.bin.unsigned
../../scripts/fpga_fw_signer/sign_fpga_fw.py target/thumbv6m-none-eabi/release/stage1.bin ../../scripts/private.pem ../../scripts/public.pem
diff -u <(hexdump -C target/thumbv6m-none-eabi/release/stage1.bin.unsigned) <(hexdump -C target/thumbv6m-none-eabi/release/stage1.bin) | grep --color -E '(\+|\-)'
st-flash --connect-under-reset write target/thumbv6m-none-eabi/release/stage1.bin 0x08002000
