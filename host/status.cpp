/*
 * Read status registers.
 */

#include <cstdio>
#include "lib/hps.h"

int status(HPS *hps, int argc, char *argv[]) {
	auto dev = hps->Device();
	for (int i = 0; i < 5; i++) {
		printf("reg %d = %04x\n", i, dev->readReg(i));
	}
	return 0;
}
