MEMORY
{
  ROM (rx)   : ORIGIN = 0x08000000, LENGTH = 16K
  RAM (xrw)  : ORIGIN = 0x20000000, LENGTH = 8K
}

/* Define sections. */
SECTIONS
{
        .text : {
                *(.vectors)     /* Vector table */
                /* crt0 runtime entry code */
                *crt*(.text)
                /* Avoid GC on crt0 with -function-sections passed. */
                KEEP(*crt*(.text))
                *(.text*)       /* Program code */
                . = ALIGN(4);
                *(.rodata*)     /* Read-only data */
                . = ALIGN(4);
        } >ROM

	. = ALIGN(4);
        _etext = .;


        /* ram, but not cleared on reset. eg:  boot/app comms */
        .noinit (NOLOAD) : {
                *(.noinit)
        } >RAM
        . = ALIGN(4);

        .data : {
                _data = .;
                *(.data*)       /* Read-write initialized data */
                . = ALIGN(4);
                *(.ramfunctions) /* functions we need/want to run from ram */
                _edata = .;
        } >RAM AT >ROM
        _data_loadaddr = LOADADDR(.data);

        .bss : {
                *(.bss*)        /* Read-write zero initialized data */
                *(COMMON)
                . = ALIGN(4);
                _ebss = .;
        } >RAM

        /*
         * The .eh_frame section appears to be used for C++ exception handling.
         * You may need to fix this if you're using C++.
         */
        /DISCARD/ : { *(.eh_frame) }

        . = ALIGN(4);
        end = .;
}

/* NOTE: Do *NOT* modify `_stack_start` unless you know what you are doing. */
PROVIDE(_stack_start = ORIGIN(RAM) + LENGTH(RAM));
