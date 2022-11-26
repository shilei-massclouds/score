/*
 * Copyright (c) 2022 Shi Lei
 *
 * Use of this source code is governed by a MIT-style license
 * that can be found in the LICENSE file or
 * at https://opensource.org/licenses/MIT
 */

OUTPUT_ARCH(riscv)
ENTRY(_start)

INCLUDE kernel_generated.ld

KERNEL_BASE = CONFIG_KERNEL_BASE;
PAGE_SIZE   = 1 << CONFIG_PAGE_SHIFT;

SECTIONS
{
    /* Beginning of entry code segment */
    . = KERNEL_BASE;
    _start = .;

    .text.entry : AT(ADDR(.text.entry) - KERNEL_BASE) {
        KEEP(*(.text.entry))
    }

    . = ALIGN(PAGE_SIZE);

    /* Start of text section */
    _stext = .;
    _text = .;
    .text : AT(ADDR(.text) - KERNEL_BASE) {
        *(.text* .sram.text)
    }
    _etext = .;

    /* Start of data section */
    . = ALIGN(PAGE_SIZE);
    _sdata = .;

    .rodata : AT(ADDR(.rodata) - KERNEL_BASE) {
        *(.rodata*)
    }
    .srodata : AT(ADDR(.srodata) - KERNEL_BASE) {
        *(.srodata*)
    }

    /* Start of bss section */
    . = ALIGN(PAGE_SIZE);
    .data : AT(ADDR(.data) - KERNEL_BASE) {
        *(.data .data.*)
    }
    .sdata : AT(ADDR(.sdata) - KERNEL_BASE) {
        /*__global_pointer$ = . + 0x800; */
        *(.sdata*)
    }

    /* End of data section */
    _edata = .;

    . = ALIGN(PAGE_SIZE);

    .bss : AT(ADDR(.bss) - KERNEL_BASE) {
        /* boot stack and boot heap which are aligned. */
        _boot_stack = .;
        . += CONFIG_STACK_SIZE;
        _boot_stack_top = .;
        _boot_heap = .;
        . += CONFIG_BOOT_HEAP_SIZE;
        _boot_heap_end = .;
        *(.bss*)
        *(COMMON)
    }
    .sbss : AT(ADDR(.sbss) - KERNEL_BASE) {
        *(.dynsbss)
        *(.sbss)
        *(.scommon)
    }

    . = ALIGN(PAGE_SIZE);
    _end = .;
}
