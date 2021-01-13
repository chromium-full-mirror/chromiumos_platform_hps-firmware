.cpu cortex-m0plus
.thumb

// FIXME(b/73553855): Get rid of this cheap crt.s implementation when we use
// cortex-m crate.

// end of 8K RAM
.word _stack_start
.word _reset
.thumb_func
_reset:
    bl entry
    b .
