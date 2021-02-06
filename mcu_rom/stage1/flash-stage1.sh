#!/bin/sh

arm-none-eabi-objcopy -O binary target/thumbv6m-none-eabi/release/stage1 target/thumbv6m-none-eabi/release/stage1.bin
st-flash --connect-under-reset write target/thumbv6m-none-eabi/release/stage1.bin 0x08002000
