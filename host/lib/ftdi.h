/*
 * Access via FTDI device.
 */
#ifndef __FTDI_H__
#define __FTDI_H__

#include "dev.h"

class FTDI : public DevInterface {
  public:
	FTDI() { }
	virtual bool read(uint8_t cmd, std::vector<uint8_t> &data);
	virtual bool write(uint8_t cmd, std::vector<uint8_t> &data);
  protected:
};

#ifndef __FTDI_H__
