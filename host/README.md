# Host utilities build and use

## Building

To build the host utilities, initially generate the build files.
This only has to be done once.
```bash
cd to-your-hps-firmware-directory
source environment
gn gen $HOSTPATH
```

To rebuild the host utilities:

```bash
ninja -C $HOSTPATH hps
```

$PATH should already be set to include the output.

## Running the host utilities

To run the utilities:

```bash
hps [ -f --bus <i2c-bus> --addr <i2c-addr> ] <command> <command arguments>
```
```-f``` selects a FTDI USB connection to the device, otherwise direct I2C is assumed.
```--bus``` defines which I2C bus to use.
```--addr``` sets the I2C peripheral address to use.

The following commands are supported:

```bash
hps -f status   # Read the common status registers and display them
hps -f cmd <value> # Send a command to the module.
hps -f dl <bank> file # Download the file to the bank selected
```
