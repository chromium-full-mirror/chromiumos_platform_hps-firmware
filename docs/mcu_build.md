# MCU build and program

## Running On Device

To build all of the MCU code:
```bash
(cd $HPS_ROOT/mcu_rom/stage0 && cargo build --release)
(cd $HPS_ROOT/mcu_rom/stage1 && cargo build --release)
(cd $HPS_ROOT/mcu_rom/application && cargo build --release)
```

Now connect the STM32G071 Nucleo-64 board to your host system and flash the
MCU with the above staged firmware in the following way (choose the one you want to flash):

```bash
cd $HPS_ROOT
./scripts/flash-mcu mcu_rom/stage0/target/thumbv6m-none-eabi/release/stage0
./scripts/flash-mcu mcu_rom/stage1/target/thumbv6m-none-eabi/release/stage1
./scripts/flash-mcu mcu_rom/application/target/thumbv6m-none-eabi/release/application
```

Finally, to see log messages run:

```bash
(cd $HPS_ROOT/mcu_rom/stage0 && cargo run)

To run the MCU application and see output:

```bash
(cd $HPS_ROOT/mcu_rom/application && cargo run)
```

The host utilities are in the main ChromeOS platform2 repo.
To build the host program:
```bash
cd ~/chromiumos    # or where ever your ChromeOS tree is
cros_sdk
sudo emerge hps-tool
```

If you are connected via a FTDI cable,
verify the I2C connection to the HPS board:

```bash
hps -f status
```

### Dev features

To enable dev (non-production) features, run with:

```bash
cargo run --features mcu_common/dev
```

This enables features that we don't want in production, but which can be useful for testing. e.g. a register that is slow to read and causes an i2c underrun.
