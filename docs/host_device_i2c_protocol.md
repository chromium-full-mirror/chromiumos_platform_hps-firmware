## Protocol overview

All communication is initiated by the host (I2C controller) and consists of
either a single write (START_WRITE...STOP), or a write_read
(START_WRITE...START_READ...STOP) operation.

All multi-byte values are big-endian.

The first byte written by the host is the command byte:

| BIT | NAME      | DESCRIPTION                   |
|:----|:----------|:------------------------------|
| 7-X | CMD       | Command is either 1 or 2 bits |
| X-0 | PARAMETER | Parameter is 7 or 6 bits      |

## CMD

| Value | FUNCTION         |
|:------|:-----------------|
| 00    | Write memory.    |
| 1     | Register Access. |
| 01    | Unused           |

# Write Memory

6 bit Parameter specifies the memory bank, followed by a 32 bit start address,
and then the data.

## Memory Banks

| Number | Bank          |                    |
|:-------|:--------------|:-------------------|
| 0      | MCU flash     | Writable in stage0 |
| 1      | FPGA gateware | Writable in stage1 |
| 2      | SOC ROM       | Writable in stage1 |

# Register Access

7 bit Parameter specifies the register number. The host either requests data
from the register (write_read operation), or data to be written to the register
follows the command (write operation). All writes are 16 bits. All documented
reads are 16 bits. Registers larger than 16 bits are supported but are currently
only used for development and debugging purposes.

| Register | Access | Stage  | Description            |
|:---------|:-------|:-------|:-----------------------|
| 0        | RO     | All    | Magic Number           |
| 1        | RO     | All    | Hardware Version       |
| 2        | RO     | All    | Common System Status   |
| 3        | WO     | All    | Common System Commands |
| 4        | RO     | All    | Application Version    |
| 5        | RO     | All    | Memory Bank Status     |
| 6        | RO     | All    | Common Error Status    |
| 7        | RW     | Appl   | Feature Enable         |
| 8        | RO     | Appl   | Feature 0 Status Bits  |
| 9        | RO     | Appl   | Feature 1 Status Bits  |
| 10       | RO     | Stage0 | Firmware Version HIGH  |
| 11       | RO     | Stage0 | Firmware Version LOW   |
| 12       | RO     | Appl   | FPGA boot count        |
| 13       | RO     | Appl   | FPGA loop count        |
| 14       | RO     | Appl   | FPGA ROM version       |
| 15       | RO     | Appl   | SPI flash status bits  |
| 16       | RO     | Appl   | Debug idx reg          |
| 17       | RO     | Appl   | Debug val reg          |

Registers numbered 100 and over are reserved for debugging and development use
and are not documented here.

## Magic Number

0x9df2

## Hardware Version

16 bits describing hardware and RO firmware version.

## Common System Status

| BIT | NAME      | DESCRIPTION                                          |
|:----|:----------|:-----------------------------------------------------|
| 15  |           |                                                      |
| 14  |           |                                                      |
| 13  |           |                                                      |
| 12  |           |                                                      |
| 11  |           |                                                      |
| 10  |           |                                                      |
| 9   | APPLREADY | Application is running, and features may be enabled. |
| 8   | STAGE1    | Stage 1 has been launched, and is now running.       |
| 7   |           |                                                      |
| 6   |           |                                                      |
| 5   | WPOFF     | Write protect pin off.                               |
| 4   | WPON      | Write protect pin on.                                |
| 3   | ANONVER   | The stage 1 image has failed to verify.              |
| 2   | AVERIFY   | The stage 1 image is verified                        |
| 1   | FAULT     | System has an unrecoverable fault.                   |
| 0   | OK        | System is operational.                               |

## Common System Commands

| BIT | NAME         | DESCRIPTION                            |
|:----|:-------------|:---------------------------------------|
| 15  | ERASE_STAGE0 | Erase first page of MCU flash (stage0) |
| 14  |              |                                        |
| 13  |              |                                        |
| 12  |              |                                        |
| 11  |              |                                        |
| 10  |              |                                        |
| 9   |              |                                        |
| 8   |              |                                        |
| 7   |              |                                        |
| 6   |              |                                        |
| 5   |              |                                        |
| 4   |              |                                        |
| 3   |              |                                        |
| 2   | LAUNCHAPP    | Launch application (from stage1)       |
| 1   | LAUNCH1      | Launch stage1 (from stage0)            |
| 0   | RESET        | Reset module to stage0                 |

## Application Version

16 bit application version number. Only present if AVERIFY is set.

## Memory Bank Ready

Bit N is set if memory bank N is available for a Memory Write request.
The host should poll this register before issuing a Memory Write command.

## Common Error Status

| BIT | NAME      | DESCRIPTION                |
|:----|:----------|:---------------------------|
| 15  |           |                            |
| 14  |           |                            |
| 13  |           |                            |
| 12  |           |                            |
| 11  |           |                            |
| 10  | SPINOTVER | SPI flash was not verified |
| 9   | BUFORUN   | Buffer overrun             |
| 8   | BUFNAVAIL | Buffer not available       |
| 7   | I2CBADREQ | A bad I2C request was made |
| 6   | SPIFLASH  | SPI flash access failed    |
| 5   | CAMERA    | Camera not functional      |
| 4   | I2CORUN   | I2C overrun                |
| 3   | I2CBERR   | I2C bus error              |
| 2   | PANIC     | A panic occurred           |
| 1   | MCUFLASH  | Error writing to MCU flash |
| 0   | I2CURUN   | I2C Underrun               |

## Feature Enable

| BIT | NAME     | DESCRIPTION      |
|:----|:---------|:-----------------|
| 15  |          |                  |
| 14  |          |                  |
| 13  |          |                  |
| 12  |          |                  |
| 11  |          |                  |
| 10  |          |                  |
| 9   |          |                  |
| 8   |          |                  |
| 7   |          |                  |
| 6   |          |                  |
| 5   |          |                  |
| 4   |          |                  |
| 3   |          |                  |
| 2   |          |                  |
| 1   | FEATURE1 | Enable feature 1 |
| 0   | FEATURE0 | Enable feature 0 |

## Feature N

| BIT | NAME  | DESCRIPTION                         |
|:----|:------|:------------------------------------|
| 15  | VALID | The feature result is valid to use. |

Bits 7-0 are the feature result.

## Boot sequence

From the perspective of the host, there are 3 boot stages: stage0, stage1 and
'Application' (Appl). Stage1 and the application are a single binary with two
different stages.

Provided no firmware updates are required, the HPS can be started by sending a
LAUNCH1 command to stage0, then once stage1 starts, by sending a LAUNCHAPP
command.

## Firmware updates

A boot sequence with firmware updates can be performed as follows:

* Reset, power cycle, or otherwise ensure that stage0 is running.
* Write new stage1/application.
* Issue RESET command
* Issue LAUNCH1 command
* Write gateware and SOC ROM.
* Issue LAUNCHAPP command
* Operate as per normal (e.g. enable a feature)
