# MCU build and program

## Running On Device

To build all of the MCU code:
```bash
(cd $HPS_ROOT/rust/mcu/stage0 && cargo build --release)
(cd $HPS_ROOT/rust/mcu/stage1_app && cargo build --release)
```

Now connect the STM32G071 Nucleo-64 board to your host system and flash the
MCU with the above staged firmware in the following way (choose the one you want to flash):

```bash
cd $HPS_ROOT
./scripts/flash-mcu rust/mcu/stage0/target/thumbv6m-none-eabi/release/stage0
./scripts/flash-mcu \
    rust/mcu/stage1_app/target/thumbv6m-none-eabi/release/stage1_app
```

Finally, to see log messages run:

```bash
(cd $HPS_ROOT/rust/mcu/stage0 && cargo run)

To run the MCU application and see output:

```bash
(cd $HPS_ROOT/rust/mcu/stage1_app && cargo run)
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

## proto2

Run `./scripts/proto2-run`

```
** Programming Finished **
** Verify Started **
** Verified OK **
** Resetting Target **
```

Once it has run. Control-C, unplug your board then plug it back in again (you only need to do that the first time you program it).
Run `./scripts/proto2-run` again.
In a separate terminal, run `./scripts/proto2-mon`.
Press enter to activate the console, then type a command `reset_mcu` and `reset_fpga`.
You should see various debug outputs get printed.

```
>> reset_mcu
MCU> INFO: Found expected flash chip
MCU> INFO: Default HM01B0 configuration applied
MCU> INFO: MCU application started. CPU is running at 60MHz
>> reset_fpga
MCU> INFO: Resetting FPGA
MCU> INFO: FPGA reported boot #1
FPGA>  (built 2021-11-23 22:31:11 UTC by xxx)
FPGA> Running classification
```

### Collecting images

Images can be collected from the HPS using the `hps-mon` tool. Sample usage:

```bash
./scripts/hps-mon --image-out ~/tmp/hps-images --image-type color
```

Once that's started, you need to connect to the FPGA via UART and press 't' to enable transfer of
images. You may also want to disable classification by pressing 'c' to speed things up. Eventually
these steps, and the need for using a UART should hopefully be removed.

You should see output like:

```
Wrote "/home/foo/tmp/hps-images/0000.raw"
```

Note, that the images will also be written as PNG in addition to raw, although this is not logged.

## Dev features

To enable dev (non-production) features, run with:

```bash
cargo run --features mcu_common/dev
```

This enables features that we don't want in production, but which can be useful for testing. e.g. a register that is slow to read and causes an i2c underrun.
