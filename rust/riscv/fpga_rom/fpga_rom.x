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
     * reset address. */
    .text_init _stext :
    {
        KEEP(*(.init));
    } > spiflash

    /* Our models are large, fixed size and during development tend to change
     * less than our code, so put them next so as to avoid having them move around. */
    .model_data :
    {
        *(.model_data)
    } > spiflash

    .text_cfu_check : ALIGN(32)
    {
        *(.text_cfu_check .text_cfu_check.*)
    } > spiflash

    /* Code for libraries. We put this first so that it's less likely to be
     * moved. */
    .text_core :
    {
        *libc.a:*(.text .text.*);
        *libgcc.a:*(.text .text.*);
        *libstdc++.a:*(.text .text.*);
        *libm.a:*(.text .text.*);
        *libriscv*:*(.text .text.*);
        *libcompiler_builtins*:*(.text .text.*);
    } > spiflash

    /* Data for libraries. We put this first so that it's less likely to be
     * moved. */
    .rodata_core :
    {
        *libc.a:*(.srodata .srodata.* .rodata .rodata.*);
        *libgcc.a:*(.srodata .srodata.* .rodata .rodata.*);
        *libstdc++.a:*(.srodata .srodata.* .rodata .rodata.*);
        *libm.a:*(.srodata .srodata.* .rodata .rodata.*);
        *libriscv*:*(.srodata .srodata.* .rodata .rodata.*);
        *libcompiler_builtins*:*(.srodata .srodata.* .rodata .rodata.*);
    } > spiflash

    .text_tflm :
    {
        *libtflite-micro.a:*(.text .text.*);
    } > spiflash

    .rodata_tflm :
    {
        *libtflite-micro.a:*(.srodata .srodata.* .rodata .rodata.*);
    } > spiflash

    /* All our other code. */
    .text :
    {
        /* Align to a SPI flash block boundary so it's less likely that this
         * section will cross a block boundary. It also means that changes
         * to TFLM will be less likely to shift this section. We fill with the
         * empty flash value (0xff) so that we can skip writing the padding. */
        FILL(0xffffffff);
        . = ALIGN(65536);

        /* By putting .init.rust here, its address (which is referenced by
         * .init) is less likely to change. We don't put it at the start of flash
         * with .init because it references our main function, which is likely to
         * change address. */
        KEEP(*(.init.rust));
        *(.text.sys .text.sys.*);

        /* Align trap table. */
        . = ALIGN(4);
        *(.trap);
        *(.trap.rust);

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
     * SPI flash. Copied from SPI flash to RAM by asm.S (from crate riscv-rt).
     *
     * We only need alignment of 4, however we set a larger alignment so that
     * small changes in bss are less likely to affect our layout
     */
    .data : ALIGN(256)
    {
        /* The address (in SPI flash) to copy from. */
        _sidata = LOADADDR(.data);
        /* The address in RAM to copy to. */
        _sdata = .;

        /* We allocate data for our libraries first so that it's less likely to move. */
        *libc.a:*(.sdata .sdata.* .data .data.*);
        *libgcc.a:*(.sdata .sdata.* .data .data.*);
        *libstdc++.a:*(.sdata .sdata.* .data .data.*);
        *libm.a:*(.sdata .sdata.* .data .data.*);
        *libriscv*:*(.sdata .sdata.* .data .data.*);
        *libcompiler_builtins*:*(.sdata .sdata.* .data .data.*);

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
