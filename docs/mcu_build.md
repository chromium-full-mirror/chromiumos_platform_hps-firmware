# MCU build and program

## Running On Device

To build both stage0 (RO) and stage1 (RW):
```bash
cd ~/src/hps-firmware/mcu_rom/stage0 && cargo build --release
cd ~/src/hps-firmware/mcu_rom/stage1 && cargo build --release
```

Now connect the STM32G071 Nucleo-64 board to your host system and flash the
MCU with the above staged firmware in the following way:

```bash
cd ~/src/hps-firmware/

./scripts/flash-mcu mcu_rom/stage0/target/thumbv6m-none-eabi/release/stage0
./scripts/flash-mcu mcu_rom/stage1/target/thumbv6m-none-eabi/release/stage1
```

Finally, to see log messages run:

```bash
cd ~/src/hps-firmware/mcu_rom/stage0 && cargo run
```
