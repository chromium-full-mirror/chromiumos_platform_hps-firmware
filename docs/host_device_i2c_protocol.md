# HPS I2C Protocol

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

## Write memory

6 bit Parameter specifies the memory bank, followed by a 32 bit start address,
and then the data.

### Memory banks

| Number | Bank          |                    |
|:-------|:--------------|:-------------------|
| 0      | MCU flash     | Writable in stage0 |
| 1      | FPGA gateware | Writable in stage1 |
| 2      | SOC ROM       | Writable in stage1 |

## Register access

7 bit Parameter specifies the register number. The host either requests data
from the register (write_read operation), or data to be written to the register
follows the command (write operation).

| Register | Access | Stage       | Description                  | Length |
|:---------|:-------|:------------|:-----------------------------|-------:|
| 0        | RO     | All         | Magic number                 | 2      |
| 1        | RO     | Stage0      | Hardware version             | 2      |
| 2        | RO     | All         | System status                | 2      |
| 3        | WO     | All         | System commands              | 2      |
| 4        | RO     | ---         | Application version (unused) | 2      |
| 5        | RO     | All         | Memory bank available        | 2      |
| 6        | RO     | All         | Error                        | 2      |
| 7        | RW     | Appl        | Enabled features             | 2      |
| 8        | RO     | Appl        | Feature 0                    | 2      |
| 9        | RO     | Appl        | Feature 1                    | 2      |
| 10       | RO     | Stage1/Appl | Firmware version (high)      | 2      |
| 11       | RO     | Stage1/Appl | Firmware version (low)       | 2      |
| 12       | RO     | Appl        | FPGA boot count              | 2      |
| 13       | RO     | Appl        | FPGA loop count              | 2      |
| 14       | RO     | Appl        | FPGA ROM version             | 2      |
| 15       | RO     | Appl        | SPI flash status bits        | 2      |
| 16       | RO     | ---         | (unused)                     | 2      |
| 17       | RO     | ---         | (unused)                     | 2      |
| 18       | RO     | Appl        | Camera configuration         | 2      |
| 19       | WO     | Appl        | Start camera test            | 2      |
| 20       | RW     | OneTimeInit | Option bytes configuration   | 2      |
| 21       | RO     | Appl        | Part IDs                     | 20     |
| 22       | RO     | Stage1/Appl | Previous crash message       | 256    |
| 23       | RO     | Appl        | FPGA crash message           | 256    |

Registers numbered 100 and over are reserved for debugging and development use
and are not documented here.

### Register 0: Magic number

0x9df2

### Register 1: Hardware version

This register is only valid while stage0 is running. At other times, it reads
zero.

The high byte describes the hardware version, only 0x01 is used.

The low byte describes the stage0 firmware version.

| Value | Meaning                                                                 |
|:------|:------------------------------------------------------------------------|
| 0x01  | Early prototyping version                                               |
| 0x02  | The stage0/stage1 boundary was moved                                    |
| 0x03  | The MP key is used for verification                                     |
| 0x04  | Launch1 now does signature check. AVERIFY and ANONVER status bits gone. |
| 0x05  | Reserved for developer versions built from source. |

### Register 2: System status

| BIT | NAME               | DESCRIPTION                                          |
|:----|:-------------------|:-----------------------------------------------------|
| 15  |                    |                                                      |
| 14  |                    |                                                      |
| 13  | ONE_TIME_INIT      | Whether the one_time_init binary is running          |
| 12  | STAGE0_PERM_LOCKED | Whether stage0 has been made permanently read-only   |
| 11  | STAGE0_LOCKED      | Whether stage0 has been made read-only               |
| 10  | CMDINPROGRESS      | A command is in-progress.                            |
| 9   | APPLREADY          | Application is running, and features may be enabled. |
| 8   | STAGE1             | Stage 1 has been launched, and is now running.       |
| 7   |                    |                                                      |
| 6   |                    |                                                      |
| 5   | WPOFF              | Write protect pin off.                               |
| 4   | WPON               | Write protect pin on.                                |
| 3   | STAGE0             | Stage 0 is running.                                  |
| 2   | DEPRECATED1        |                                                      |
| 1   | FAULT              | System has an unrecoverable fault.                   |
| 0   | OK                 | System is operational.                               |

### Register 3: System commands

| BIT | NAME            | DESCRIPTION                                      |
|:----|:----------------|:-------------------------------------------------|
| 15  | ERASE_STAGE0    | Erase stage0 (one_time_init)                     |
| 14  | WRITE_TEST_DATA | Write SPI flash test data                        |
| 13  |                 |                                                  |
| 12  |                 |                                                  |
| 11  |                 |                                                  |
| 10  |                 |                                                  |
| 9   |                 |                                                  |
| 8   |                 |                                                  |
| 7   |                 |                                                  |
| 6   |                 |                                                  |
| 5   |                 |                                                  |
| 4   | ERASE_SPIFLASH  | Erase SPI flash (FPGA bitstream and application) |
| 3   | ERASE_STAGE1    | Erase RW region of MCU flash (stage1)            |
| 2   | LAUNCHAPP       | Launch application (from stage1)                 |
| 1   | LAUNCH1         | Launch stage1 (from stage0)                      |
| 0   | RESET           | Reset module to stage0                           |

### Register 5: Memory bank available

Bit N is set if memory bank N is available for a Memory Write request.
The host should poll this register before issuing a Memory Write command.

### Register 6: Error

If an unrecoverable error has occurred (indicated by the FAULT bit in the
system status register), then the error register contains a value that
identifies the first error that occurred.

See [`rust/mcu_common/src/errors.rs`](../rust/mcu_common/src/errors.rs)
for the mapping from values to error names and descriptions.

### Register 7: Enabled features

| BIT | NAME      | DESCRIPTION      |
|:----|:----------|:-----------------|
| 15  |           |                  |
| 14  |           |                  |
| 13  |           |                  |
| 12  |           |                  |
| 11  |           |                  |
| 10  |           |                  |
| 9   |           |                  |
| 8   |           |                  |
| 7   |           |                  |
| 6   |           |                  |
| 5   |           |                  |
| 4   |           |                  |
| 3   |           |                  |
| 2   |           |                  |
| 1   | FEATURE1  | Enable feature 1 |
| 0   | FEATURE0  | Enable feature 0 |

### Registers 8-9: Feature N

| BITS | NAME   | DESCRIPTION                         |
|:-----|:-------|:------------------------------------|
| 15   | USABLE | The feature result is valid to use. |
| 14-8 | COUNT  | Wrapping result counter.            |
| 7-0  | RESULT | Signed 8 bit score                  |

### Register 18: Camera configuration

| BIT | NAME         | DESCRIPTION                            |
|:----|:-------------|:---------------------------------------|
| 15  |              |                                        |
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
| 2   |              |                                        |
| 1   | ROTATION     | Rotation bit 1                         |
| 0   | ROTATION     | Rotation bit 0                         |

Supported rotations:

| VALUE | ROTATION   | DESCRIPTION                            |
|:------|:-----------|----------------------------------------|
| 0     | UPRIGHT    | No rotation required                   |
| 1     | CLOCKWISE  | Image needs 90 clockwise rotation      |
| 2     |            |                                        |
| 3     |            |                                        |

### Register 21: Part IDs

The Part IDs register consists of multiple 32 bit words as follows:

| BYTE_OFFSET | NAME         | DESCRIPTION                                  |
|:------------|:-------------|:---------------------------------------------|
| 0           | MCU_ID       | See [RM0444] section 40.10.1                 |
| 4           | FPGA_ID      | Not yet implemented. Reserved for future use |
| 8           | CAMERA_ID    | Camera part ID                               |
| 12          | SPI_FLASH_MN | SPI flash manufacturer                       |
| 16          | SPI_FLASH_PI | SPI flash part ID                            |

[RM0444]: https://www.st.com/resource/en/reference_manual/rm0444-stm32g0x1-advanced-armbased-32bit-mcus-stmicroelectronics.pdf

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
* Erase MCU RW flash region.
* Write new stage1/application into MCU RW flash region.
* Issue RESET command
* Issue LAUNCH1 command
* Erase SPI flash.
* Write FPGA bitstream and application into SPI flash.
* Issue LAUNCHAPP command
* Operate as per normal (e.g. enable a feature)
