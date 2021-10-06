# FPGA SoC build and program

## Simulation - no hardware needed

To simulate the SoC on your local machine (no HPS hardware needed), run:

```bash
./scripts/pyrun -m soc.simulate
```

The simulator is slow while running but very quick to start, so useful for
testing many kinds of code changes.


## Running On Device

To build the bitstream and software, then program the device:
```bash
./scripts/soc-load --build
```

Finally, connect a UART and communicate with the running program:

```bash
./scripts/term /dev/ttyUSB0
```
(replace `/dev/ttyUSB0` with the appropriate device name)

