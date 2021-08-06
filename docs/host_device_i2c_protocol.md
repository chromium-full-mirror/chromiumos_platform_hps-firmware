All multi-byte values are big-endian.
There are 3 boot stages, stage0, stage1, and 'Application' (Appl).

The first byte is the command byte:

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

| Number | Bank      |                    |
|:-------|:----------|:-------------------|
| 0      | MCU flash | Writable in stage0 |
| 1      | SPI flash | Writable in stage1 |

# Register Access

7 bit Parameter specifies the register number.
The host either requests data from the register,
or data to be written to the register follows the command.
All writes and reads are 16 bits.

| Register | Access | Stage | Description            |
|:---------|:-------|:------|:-----------------------|
| 0        | RO     | All   | Magic Number           |
| 1        | RO     | All   | Hardware Version       |
| 2        | RO     | All   | Common System Status   |
| 3        | WO     | All   | Common System Commands |
| 4        | RO     | All   | Application Version    |
| 5        | RO     | All   | Memory Bank Status     |
| 6        | RO     | All   | Common Error Status    |

## Magic Number

0x9df2

## Hardware Version

16 bits describing hardware and RO firmware version.

## Common System Status

| BIT | NAME      | DESCRIPTION                                         |
|:----|:----------|:----------------------------------------------------|
| 15  |           |                                                     |
| 14  |           |                                                     |
| 13  |           |                                                     |
| 12  |           |                                                     |
| 11  | SPINOTVER | FPGA SPI flash has failed verification.             |
| 10  | SPIVERIFY | FPGA SPI flash is verified.                         |
| 9   |           |                                                     |
| 8   | APPLRUN   | Post stage 0 has been launched, and is now running. |
| 7   |           |                                                     |
| 6   |           |                                                     |
| 5   | WPOFF     | Write protect pin off.                              |
| 4   | WPON      | Write protect pin on.                               |
| 3   | ANONVER   | The stage 1 image has failed to verify.             |
| 2   | AVERIFY   | The stage 1 image is verified                       |
| 1   | FAULT     | System has an unrecoverable fault.                  |
| 0   | OK        | System is operational.                              |

## Common System Commands

| BIT | NAME   | DESCRIPTION            |
|:----|:-------|:-----------------------|
| 15  |        |                        |
| 14  |        |                        |
| 13  |        |                        |
| 12  |        |                        |
| 11  |        |                        |
| 10  |        |                        |
| 9   |        |                        |
| 8   |        |                        |
| 7   |        |                        |
| 6   |        |                        |
| 5   |        |                        |
| 4   |        |                        |
| 3   |        |                        |
| 2   | ENABLE | Enable the next stage. |
| 1   | LAUNCH | Launch stage1          |
| 0   | RESET  | Reset module to stage0 |

## Application Version

16 bit application version number. Only present if AVERIFY is set.

## Memory Bank Ready

Bit N is set if memory bank N is available for a Memory Write request.
The host should poll this register before issuing a Memory Write command.

## Common Error Status

| BIT | NAME     | DESCRIPTION                |
|:----|:---------|:---------------------------|
| 15  |          |                            |
| 14  |          |                            |
| 13  |          |                            |
| 12  |          |                            |
| 11  |          |                            |
| 10  |          |                            |
| 9   |          |                            |
| 8   |          |                            |
| 7   |          |                            |
| 6   |          |                            |
| 5   |          |                            |
| 4   | I2CORUN  | I2C overrun                |
| 3   | I2CBERR  | I2C bus error              |
| 2   | PANIC    | A panic occurred           |
| 1   | MCUFLASH | Error writing to MCU flash |
| 0   | I2CURUN  | I2C Underrun               |
