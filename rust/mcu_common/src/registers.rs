// Copyright 2021 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use num_enum::TryFromPrimitive;

#[derive(Copy, Clone, Debug, Eq, PartialEq, TryFromPrimitive)]
#[repr(u8)]
pub enum Register {
    /// RO: A magic value that can help to confirm that the device is a HPS.
    Magic = 0,

    /// RO: Hardware + stage0 version.
    HardwareVersion = 1,

    /// RO: System status.
    SystemStatus = 2,

    /// WO: Performs a system command.
    Command = 3,

    /// RO: Application version
    ApplicationVersion = 4,

    /// RO: Which memory banks can currently be written.
    MemoryBankAvailable = 5,

    /// RO: The first error that has occurred, if any. See errors.rs for values.
    Error = 6,

    /// WO: Which application features are enabled.
    EnabledFeatures = 7,

    /// RO: Person detection status. See docs/host_device_i2c_protocol.md for
    /// breakdown of individual bits.
    UserPresentStatus = 8,

    /// RO: Second person detection status. See docs/host_device_i2c_protocol.md for
    /// breakdown of individual bits.
    SecondPersonStatus = 9,

    /// RO: Version register for hpsd.
    FirmwareVersionHigh = 10,
    FirmwareVersionLow = 11,

    /// RO: Number of times the FPGA soft CPU has reported to us that it's starting.
    FpgaBootCount = 12,

    /// RO: Number of times the FPGA soft CPU has run its main loop (since it last
    /// booted).
    FpgaLoopCount = 13,

    /// RO: FPGA ROM version. This version has no semantic meaning. It is
    /// incremented at the whim of developers and is mostly for debugging purposes.
    FpgaRomVersion = 14,

    /// RO: SPI flash status registers as read at startup.
    SpiFlashStatus = 15,

    /// RO: Configuration bits for camera.
    CameraConfig = 18,

    /// RW: Number of camera communication test iterations to perform. Write a
    /// non-zero value then wait until the value read reaches zero. If any
    /// communication errors occur, the camera error bit will be set.
    CameraTestIterations = 19,

    /// RW: Encoded OptionBytesConfigRequest. Supported by the one_time_init binary.
    /// Writing a request to this register initializes the flash option bytes.
    /// Reading this register, returns which configuration options have already been
    /// applied.
    ConfigurationOptionBytes = 20,

    /// RO: Part IDs. This register contains the content of the PartIds struct.
    PartIds = 21,

    /// RO: A crash report from the previous boot. Maximum length is 256 bytes.
    /// Payload is everything prior to the first null byte. Reading this register
    /// clears it.
    PreviousCrash = 22,

    /// RO: An FPGA crash report. Maximum length is 256 bytes. Payload is
    /// everything prior to the first null byte. Reading this register clears
    /// it.
    FpgaCrash = 23,

    /// RO: Reads will be sufficiently slow to cause an underrun.
    #[cfg(feature = "dev")]
    SlowRead = 100,

    /// RO: Whether there's a block of image data available to read.
    #[cfg(feature = "image-transfer")]
    ImageDataAvailable = 101,

    /// RO: Image data. Always 63 bytes.
    #[cfg(feature = "image-transfer")]
    ImageData = 102,

    /// RW: Doesn't do anything unless you change the code to make it do something.
    #[cfg(feature = "dev")]
    Debug1 = 111,

    /// RW: Doesn't do anything unless you change the code to make it do something.
    #[cfg(feature = "dev")]
    Debug2 = 112,

    /// RW: Doesn't do anything unless you change the code to make it do something.
    #[cfg(feature = "dev")]
    Debug3 = 113,
}
