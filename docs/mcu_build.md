# MCU build and program

## Running On Device

To build all of the MCU code:
```bash
(cd $HPS_ROOT/mcu_rom/stage0 && cargo build --release)
(cd $HPS_ROOT/mcu_rom/stage1 && cargo build --release)
(cd $HPS_ROOT/mcu_rom/stm32g0_application && cargo build --release)
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
(cd $HPS_ROOT/mcu_rom/stm32g0_application && cargo run)
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

## proto0

### Connecting proto0

Connect ST/Link via a 1.8V converter in this way:

ST/Link | proto0
--------|-------
 GND    | J6-8
 SWDIO  | J6-9
 SWDCLK | J6-10

The UART is connected via J7:

UART    | proto0
--------|--------------
 GND    | J7-12
 RX     | J7-8 (MCU RX)
 TX     | J7-9 (MCU TX)

![Proto0 Top and Bottom](./proto0-layout.jpg "Proto0 layout")

### Building and Running on proto0

By default, the MCU application builds for the stm32g071, however if you'd like to run it on
proto0, which has an stm32g031, you can do so with the following command.

```bash
cs mcu_rom/stm32g0_application && \
cargo build --no-default-features --features proto0,defmt-default && \
probe-run --probe 0483:3748 --chip stm32g031 ./target/thumbv6m-none-eabi/debug/stm32g0_application
```

## Dev features

To enable dev (non-production) features, run with:

```bash
cargo run --features mcu_common/dev
```

This enables features that we don't want in production, but which can be useful for testing. e.g. a register that is slow to read and causes an i2c underrun.
