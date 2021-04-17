// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

/*
 * FTDI device interface layer.
 * FTDI APP note AN_255 used as reference.
 */
#include <iostream>
#include <iomanip>
#include <unistd.h>

#include "ftdi.h"

namespace {

const int kTimeoutMS = 500;               // MS timeout
const int kResetDelay = 10;               // MS delay after reset.
const uint16_t kClockDivisor = (300/2-1); // Clock divisor (100KHz)

// Commands to FTDI module.
enum {
    BYTE_OUT_RISING = 0x10,
    BYTE_OUT_FALLING = 0x11,
    BIT_OUT_RISING = 0x12,
    BIT_OUT_FALLING = 0x13,
    BYTE_IN_RISING = 0x20,
    BIT_IN_RISING = 0x22,
    BYTE_IN_FALLING = 0x24,
    BIT_IN_FALLING = 0x26,
    SET_PINS = 0x80,             // Write to ADBUS 0-7
    FLUSH = 0x87,
};

// ADBUS0/ADBUS1 bits for I2C I/O
enum {
    SCK = 1,
    SDATA = 2,
    GPIO = 8,   // For debugging.
};


// Set the state of the I/O pins.
void i2c_pins(std::vector<uint8_t> &b, uint8_t val, uint8_t dir) {
    b.push_back(SET_PINS);
    b.push_back(val);
    b.push_back(dir|GPIO);
}

// Add a Start sequence to the buffer.
void i2c_start(std::vector<uint8_t> &b) {
    for (auto i = 0; i < 10; i++) {
        i2c_pins(b, SCK|SDATA, SCK|SDATA);    // Let line be pulled up.
    }
    for (auto i = 0; i < 10; i++) {
        i2c_pins(b, SCK, SCK|SDATA);
    }
    for (auto i = 0; i < 10; i++) {
        i2c_pins(b, 0, SCK|SDATA);
    }
}

// Add a Stop sequence to the buffer.
void i2c_stop(std::vector<uint8_t> &b) {
    for (auto i = 0; i < 10; i++) {
        i2c_pins(b, 0, SCK|SDATA);
    }
    for (auto i = 0; i < 10; i++) {
        i2c_pins(b, SCK, SCK|SDATA);
    }
    for (auto i = 0; i < 10; i++) {
        i2c_pins(b, SCK|SDATA, SCK|SDATA);
    }
    i2c_pins(b, SCK|SDATA, 0);
}

}  // namespace

namespace hps {

bool Ftdi::Init() {
    FT_STATUS   ftStatus;
    FT_DEVICE    ftDevice;
    char buf[64];

    ftStatus = FT_ListDevices(0, buf, FT_LIST_BY_INDEX|FT_OPEN_BY_SERIAL_NUMBER);
    if (ftStatus != FT_OK) {
        return false;
    }
    std::cout << "FTDT Serial number: " << buf << std::endl;
    ftStatus = FT_Open(0, &this->ftHandle);
    if (ftStatus != FT_OK) {
        std::cerr << "FTDI open error: " << ftStatus << std::endl;
        return false;
    }
    ftStatus = FT_GetDeviceInfo(this->ftHandle, &ftDevice, NULL, NULL, NULL, NULL);
    if (ftStatus != FT_OK) {
        std::cerr << "FTDI GetDevice error: " << ftStatus << std::endl;
        return false;
    }
    /*
     * Confirm it is the correct device type.
     */
    if (FT_DEVICE_2232H != ftDevice) {
        std::cerr << "FTDI unknown device type (expected " << FT_DEVICE_2232H;
        std::cerr << ", got " << ftDevice << ")" << std::endl;
        return false;
    }
    ftStatus = FT_ResetDevice(ftHandle);
    // Flush read queue.
    std::vector<uint8_t> rdq;
    this->ft_get(rdq);
    // Set up parameters of module.
    if (FT_SetUSBParameters(ftHandle, 65536, 65535) != FT_OK ||
        FT_SetChars(ftHandle, false, 0, false, 0) != FT_OK ||
        FT_SetTimeouts(ftHandle, 5000, 5000) != FT_OK ||
        FT_SetLatencyTimer(ftHandle, 16) != FT_OK ||
        FT_SetBitMode(ftHandle, 0x0, 0x00) != FT_OK ||
        FT_SetBitMode(ftHandle, 0x0, 0x02) != FT_OK) {
        std::cerr << "FTDI: init error" << std::endl;
        return false;
    }
    usleep(50*1000);
    this->ft_get(rdq);
    // Device opened successfully, verify MPSSE mode.
    std::vector<uint8_t> tx(1);
    tx[0] = 0xAA;
    if (this->ft_put(tx) != tx.size()) {
        std::cerr << "FTDI: verify write error" << std::endl;
        return false;
    }
    if (!this->ft_read(2, rdq) || rdq[0] != 0xFA || rdq[1] != 0xAA) {
        std::cerr << "FTDI: verify read error" << std::endl;
        return false;
    }
    tx.clear();
    // Init MPSSE settings.
    tx.push_back(0x8A);     // Disable divide/5 clock mode
    tx.push_back(0x97);     // Disable adaptive clocking
    tx.push_back(0x8C);     // Enable 3 phase clocking
    if (this->ft_put(tx) != tx.size()) {
        std::cerr << "FTDI: MPSEE setting error" << std::endl;
        return false;
    }
    tx.clear();
    i2c_pins(tx, SCK|SDATA, SCK);
    tx.push_back(0x86);     // Set clock divisor
    tx.push_back(kClockDivisor & 0xFF);
    tx.push_back(kClockDivisor >> 8);
    if (this->ft_put(tx) != tx.size()) {
        std::cerr << "FTDI: MPSEE clock setting error" << std::endl;
        return false;
    }
    usleep(20*1000);
    tx.clear();
    tx.push_back(0x85);     // Turn off loopback.
    if (this->ft_put(tx) != tx.size()) {
        std::cerr << "FTDI: loopback disable error" << std::endl;
        return false;
    }
    usleep(20*1000);
    return true;
}

bool Ftdi::read(uint8_t cmd, std::vector<uint8_t> &data) {
    if (data.size() == 0) {
        return false;
    }
    std::vector<uint8_t> b;
    // Flush anything in the read queue.
    this->ft_get(b);
    b.clear();
    i2c_start(b);
    if (!this->ft_sendbyte(this->address, b)) {
        this->i2c_reset();
        return false;
    }
    if (!this->ft_sendbyte(cmd, b)) {
        this->i2c_reset();
        return false;
    }
    b.clear();
    i2c_start(b);
    if (!this->ft_sendbyte(this->address | 1, b)) {
        this->i2c_reset();
        return false;
    }
    for (size_t i = 0; i < data.size(); i++) {
        if (!this->ft_readbyte(&data[i], i == (data.size() - 1))) {
            this->i2c_reset();
            return false;
        }
    }
    return true;
}

bool Ftdi::write(uint8_t cmd, const std::vector<uint8_t> &data) {
    std::vector<uint8_t> b;
    this->ft_get(b);
    b.clear();
    i2c_start(b);
    if (!this->ft_sendbyte(this->address, b)) {
        this->i2c_reset();
        return false;
    }
    b.clear();
    if (!this->ft_sendbyte(cmd, b)) {
        this->i2c_reset();
        return false;
    }
    for (auto it = data.begin(); it != data.end(); ++it) {
        b.clear();
        if (!this->ft_sendbyte(*it, b)) {
            this->i2c_reset();
            return false;
        }
    }
    b.clear();
    i2c_stop(b);
    if (this->ft_put(b) != b.size()) {
        return false;
    }
    return true;
}

// Read a block of data from the FTDI chip.
// A timeout is used in case it hangs.
bool Ftdi::ft_read(size_t count, std::vector<uint8_t> &input) {
    input.clear();
    std::vector<uint8_t> buf;
    int timeout = kTimeoutMS;
    while (count > 0) {
        // Read whatever is available.
        if (!this->ft_get(buf)) {
            return false;
        }
        // Append to input buffer.
        if (buf.size() != 0) {
            if (buf.size() > count) {
                // Hmm extra data...
                buf.resize(count);
            }
            input.insert(input.end(), buf.begin(), buf.end());
            count -= buf.size();
        } else {
            // No data available, sleep for a while
            // and try again.
            usleep(1000);   // Sleep for 1 ms
            if (--timeout <= 0) {
                return false;
            }
        }
    }
    return true;
}

// Send a byte to the I2C bus and wait for an ack/nak.
bool Ftdi::ft_sendbyte(uint8_t data, std::vector<uint8_t> &b) {
    // SDA/SCLK low.
    i2c_pins(b, 0, SCK|SDATA);
    b.push_back(BIT_OUT_FALLING);
    b.push_back(0x07);
    b.push_back(data);
    // Switch to SDA input to read ack/nak.
    i2c_pins(b, 0, SCK);
    b.push_back(BIT_IN_RISING);
    b.push_back(0x00);
    b.push_back(FLUSH);
    if (this->ft_put(b) != b.size()) {
        return false;
    }
    b.clear();
    std::vector<uint8_t> in;
    // Check for nak.
    if (!this->ft_read(1, in) || (in[0] & 0x01) != 0x00) {
        return false;
    }
    return true;
}

// Read a byte from the I2C and send a NAK/ACK in response.
bool Ftdi::ft_readbyte(uint8_t *result, bool nak) {
    std::vector<uint8_t> b;
    // SCK out/low, SDA in
    i2c_pins(b, 0, SCK);
    b.push_back(BIT_IN_RISING);
    b.push_back(0x07);
    i2c_pins(b, 0, SCK|SDATA);
    b.push_back(BIT_OUT_FALLING);
    b.push_back(0x00);
    b.push_back(nak ? 0x80 : 0x00);
    i2c_pins(b, 0, SCK);
    b.push_back(FLUSH);
    if (this->ft_put(b) != b.size()) {
        return false;
    }
    // Read byte.
    b.resize(1);
    if (!this->ft_read(b.size(), b)) {
        return false;
    }
    *result = b[0];
    if (nak) {
        // Send Stop.
        b.clear();
        i2c_stop(b);
        if (this->ft_put(b) != b.size()) {
            return false;
        }
    }
    return true;
}

// Read from the module whatever data is available.
bool Ftdi::ft_get(std::vector<uint8_t> &get) {
    DWORD actual = 0;
    DWORD in_q = 0;
    FT_STATUS ftStatus;

    ftStatus = FT_GetQueueStatus(this->ftHandle, &in_q);
    if (ftStatus != FT_OK) {
        get.clear();
        return false;
    }
    // Nothing to read, return empty vector.
    if (in_q == 0) {
        get.clear();
        return true;
    }
    get.resize(in_q);
    ftStatus = FT_Read(this->ftHandle, &get[0], in_q, &actual);
    if (ftStatus == FT_OK) {
        get.resize(actual);
        return true;
    } else {
        get.clear();
        return false;
    }
}

// Write the data to the module..
size_t Ftdi::ft_put(const std::vector<uint8_t> &output) {
    DWORD actual = 0;
    FT_Write(this->ftHandle, const_cast<uint8_t *>(&output[0]), output.size(), &actual);
    return size_t(actual);
}

// Reset the state of the bus to idle.
void Ftdi::i2c_reset() {
    std::vector<uint8_t> b;
    i2c_stop(b);
    this->ft_put(b);
    usleep(kResetDelay*1000);
}

}  // namespace hps
