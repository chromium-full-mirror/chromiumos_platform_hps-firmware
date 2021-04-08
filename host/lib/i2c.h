/*
 * I2C device handler.
 */
#ifndef __I2C_H__
#define __I2C_H__

#include <stdint.h>
#include <string>

#include "dev.h"

struct i2c_msg;

class I2CDev : public DevInterface {
  public:
	I2CDev(int bus, int address);
	int Open();
	virtual bool read(uint8_t cmd, std::vector<uint8_t> &data);
	virtual bool write(uint8_t cmd, std::vector<uint8_t> &data);
  protected:
	bool ioc(struct i2c_msg *msg, uint count);
	int bus;
	int address;
	int fd;
	std::string name;
};

#endif // __I2C_H__
