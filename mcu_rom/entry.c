/**
 * Quick and dirty LED flasher on GPIO A1 pin.
 * Nothing much to see here, just sanity check hw, please
 * delete when we have something more useful to hop to.
 */

#include <stdint.h>

// register address
#define RCC_BASE      0x40021000
#define GPIOA_BASE    0x50000000
#define RCC_IOPENR    *(volatile uint32_t *)(RCC_BASE   + 0x34)
#define GPIOA_ODR     *(volatile uint32_t *)(GPIOA_BASE + 0x14)

//#define GPIOA_BSRR    *(volatile uint32_t *)(GPIOA_BASE + 0x???)
#define GPIOA_MODER   *(volatile uint32_t *)(GPIOA_BASE + 0)
#define GPIOA_OTYPER  *(volatile uint32_t *)(GPIOA_BASE + 0x04)
#define GPIOA_PUPDR   *(volatile uint32_t *)(GPIOA_BASE + 0x0C)

// bit fields
#define RCC_IOPCEN   (1<<4) // 0b10000
#define GPIOA1       (1UL<<1)
#define PUPDR1_PU    (1UL<<2) // 0b0100


// Peripheral clock enable registers
// RCC_APBENRx

// IO port clock enable register (RCC_IOPENR)
// offset 0x34
#define RCC_GPIOA_EN (1UL<<0) // bit 0

void entry(void)
{
	RCC_IOPENR |= RCC_GPIOA_EN;

	GPIOA_MODER &= 0xFFFFFFF7; // MODER1[1:0] = 01 GPIO o/p i.e., 0b01XX
	GPIOA_PUPDR |= PUPDR1_PU; // PUPDR1[1:0] = 01 pull-up. i.e., 0b0100

	while(1)
	{
		// Drive LED at 1/10th of full brightness
		for (int i = 0; i < 50000; i++) {
			GPIOA_ODR |=  GPIOA1;
			GPIOA_ODR &= ~GPIOA1;
			for (int j = 0; j < 10; j++);
		}
		GPIOA_ODR &= ~GPIOA1;
		for (int i = 0; i < 500000; i++); // arbitrary delay
	}
}
