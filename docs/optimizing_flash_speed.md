# Optimizing FPGA ROM flash layout

This document describes how to optimize the speed at which you can write a new
fpga_rom during development.

When experimenting with code in fpga_rom, it can be annoying to have to wait for
the new ROM to be written to SPI flash. Our write speed, even utilizing both I2C
and SWD is only about 30KiB/s.

We optimize our link order so that code that we change the most doesn't affect
the placement of code that we don't change as much.

The code that we change the most when experimenting tends to be fpga_app and
associated crates, followed by some bits of TFLM. So we put these towards the
end.

To determine how well optimized the link order is, run `./scripts/fpga-rom-run`,
then make a representative change - e.g. comment out a println in the Rust code,
then rerun `./scripts/fpga-rom-run`. On the second run, you're looking to see
how many blocks needed to be written. e.g.:

```
Need to write 2 of 14 blocks
```

If the number of blocks is higher than you'd like, dump the assembly of the ELF
file with and without your println commented.

```
/opt/hps-sdk/bin/riscv64-unknown-elf-objdump -d \
   ./rust/riscv/target/riscv32i-unknown-none-elf/release/fpga_rom >~/tmp/a.S
```

Then diff the two assembly files:

```
meld ~/tmp/{a,b}.S
```

Find the first line that is different and see what symbol it's referring to.
That symbol has moved. Find what input file it's defined in and move it earlier
in the link order.

You can find what symbols a file defines by running `nm --defined-only` on it.

You can find all input files to the linker by running a command like the
following:

```
strace -f --string-limit=1000 ./scripts/build-fpga-rom-dev 2>&1 | \
    grep openat | grep -v ENOENT | grep -E '(\.(a|rlib|o)")' | cut -d'"' -f2
````

The diff may contain lots of changes due to different debug symbol names. e.g.:

```diff
-202acb1c:	fffc8d13          	addi	s10,s9,-1 # ffff <.LLST153+0x26>
+202acb1c:	fffc8d13          	addi	s10,s9,-1 # ffff <.LLST148+0x27>
```

Note that the word (0xfffc8d13) hasn't changed, just the symbol name and offset.
To make finding actual differences easier, you can temporarily set `debug =
false` in Cargo.toml.
