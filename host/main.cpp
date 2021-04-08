/*
 * Main command program.
 */

#include <cstdio>
#include <stdlib.h>
#include <getopt.h>
#include <string.h>

#include "lib/hps.h"
#include "lib/i2c.h"

int status(HPS *, int, char *argv[]);
int command(HPS *, int, char *argv[]);

static struct {
	const char *cmd;
	int (*func)(HPS *, int, char *[]);
	const char *desc;
} commands[] = {
{	"status", status, "Dump status registers" },
{	"cmd", command, "Send command to HPS" },
};

int main(int argc, char *argv[]) {
	int c;
	int bus = 2;
	int addr = 0x30;

	while (true) {
		int option_index = 0;
		static struct option long_options[] = {
			{"bus",		required_argument, 0,  'b' },
			{"addr",	required_argument, 0,  'a' },
			{0,         0,                 0,  0 }
		};
		c = getopt_long(argc, argv, "a:b:",
				long_options, &option_index);
		if (c == -1) {
			break;
		}
		switch (c) {
		case 'a':
			addr = atoi(optarg);
			break;

		case 'b':
			bus = atoi(optarg);
			break;

		default:
			return 1;
		}
	}
	if (optind >= argc) {
		printf("Missing command - valid commands are:\n");
		for (uint i = 0; i < sizeof(commands)/sizeof(commands[0]); i++) {
			printf(" %s - %s\n", commands[i].cmd, commands[i].desc);
		}
		return 1;
	}
	// TODO: Allow selection of FTDI.
	auto dev = new I2CDev(bus, addr);
	if (dev->Open() < 0) {
		return 1;
	}
	auto hps = new HPS(dev);
	for (uint i = 0; i < sizeof(commands)/sizeof(commands[0]); i++) {
		if (strcmp(argv[optind], commands[i].cmd)==0) {
			return commands[i].func(hps, argc - optind + 1, &argv[optind]);
		}
	}
	printf("%s: Unknown command (%s)\n", argv[0], argv[optind]);
	return 1;
}
