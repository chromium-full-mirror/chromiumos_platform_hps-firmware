# FPGA SoC build and program

## Simulation - no hardware needed

To simulate the SoC on your local machine (no HPS hardware needed), run:

```bash
./scripts/pyrun soc/simulate.py
```

The simulator is slow while running but very quick to start, so useful for
testing many kinds of code changes.


## Running On Device

To build the bitstream and program:
```bash
./scripts/pyrun soc/hps_soc.py --build
```

`--build` is only needed to rebuild of the bitstream. If the bitstream has not
changed, omit it for a faster build.

Now connect the proto0 board to the SPI and flash it with the bitstream and program:

```bash
./scripts/prog build/hps_platform/gateware/hps_platform.bit bitstream

./scripts/prog build/hps_platform/software/bios/bios.bin program
```

Finally, connect a UART and communicate with the running program:

```bash
./scripts/pyrun third_party/python/litex/litex/tools/litex_term.py /dev/ttyUSB1
```

