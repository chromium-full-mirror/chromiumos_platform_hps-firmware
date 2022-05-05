PROVIDE(_stack_size = 20K);
PROVIDE(_stack_start = ORIGIN(sram) + _stack_size);

/* newlib seems to do two calls to sbrk. One of 1048 bytes and one of 3396. So
 * we need at least 4444 bytes of heap. */
PROVIDE(_heap_size = 5K);

SECTIONS {
    /* Symbols that we don't want. */
    /DISCARD/ :
    {
        *(.init_array)
        *(.gcc_except_table.*)
        *(.eh_frame*)
        abort(*)
    }

    /* Our first section in spiflash is located at _stext, which is the CPU
     * reset address. We also put our trap table here. */
    .text_init _stext :
    {
        KEEP(*(.init));
        KEEP(*(.init.rust));

        /* Align trap table. */
        . = ALIGN(4);
        (*(.trap));
        (*(.trap.rust));
    } > spiflash

    /* Our models are large, fixed size and during development tend to change
     * less than our code, so put them next so as to avoid having them move around. */
    .model_data :
    {
        *(.model_data)
    } > spiflash

    /* All our other code. */
    .text :
    {
        *(.text .text.*);
    } > spiflash

    .rodata : ALIGN(4)
    {
        *(.srodata .srodata.*);
        *(.rodata .rodata.*);
    } > spiflash

    /* Our stack is the first thing in RAM so that if we overflow our stack,
     * we'll access invalid memory addresses and hopefully notice sooner than if we
     * overwrote other data. */
    .stack (NOLOAD) :
    {
        _estack = ORIGIN(sram);
        . = ABSOLUTE(_stack_start);
        _sstack = .;
    } > sram

    /* RAM that will be initialized to 0 by asm.S (from crate risc-rt). */
    .bss (NOLOAD) :
    {
        _sbss = .;
        *(.sbss .sbss.* .bss .bss.*);
        . = ALIGN(4);
        _ebss = .;
    } > sram

    /* RAM that needs specific non-zero values. Addresses are in RAM, data is in
     * SPI flash. Copied from SPI flash to RAM by asm.S (from crate riscv-rt). */
    .data : ALIGN(4)
    {
        /* The address (in SPI flash) to copy from. */
        _sidata = LOADADDR(.data);
        /* The address in RAM to copy to. */
        _sdata = .;
        *(.sdata .sdata.* .data .data.*);
        . = ALIGN(4);
        /* The address in RAM of the end. */
        _edata = .;
    } > sram AT > spiflash

    .heap (NOLOAD) :
    {
        _heap_start = .;
        . += _heap_size;
        _heap_end = .;
    } > sram

    /* The arena is a 256KB section region containing the TfLM tensor arena. */
    .arena (NOLOAD) :
    {
        _farena = .;
        *(.arena)
        _earena = .;
    } > arena
}

/* Needed by libriscv_rt */
PROVIDE(_max_hart_id = 0);
PROVIDE(_mp_hook = default_mp_hook);
PROVIDE(_setup_interrupts = default_setup_interrupts);
PROVIDE(__pre_init = default_pre_init);

/* We only have a single core (hart). Pretend that our per-hart stack is 0
 * bytes, crt0 just uses _start_stack as provided above. */
PROVIDE(_hart_stack_size = 0);

PROVIDE(MachineExternal = DefaultInterruptHandler);
PROVIDE(MachineSoft = DefaultInterruptHandler);
PROVIDE(MachineTimer = DefaultInterruptHandler);
PROVIDE(SupervisorExternal = DefaultInterruptHandler);
PROVIDE(SupervisorSoft = DefaultInterruptHandler);
PROVIDE(SupervisorTimer = DefaultInterruptHandler);
PROVIDE(UserExternal = DefaultInterruptHandler);
PROVIDE(UserSoft = DefaultInterruptHandler);
PROVIDE(UserTimer = DefaultInterruptHandler);

PROVIDE(DefaultHandler = DefaultInterruptHandler);
PROVIDE(ExceptionHandler = DefaultExceptionHandler);
