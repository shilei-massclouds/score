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

    /* Start of text section */
    _text_start = .;

    .text.entry : AT(ADDR(.text.entry) - KERNEL_BASE) {
        KEEP(*(.text.entry))
    }

    . = ALIGN(PAGE_SIZE);

    .text : AT(ADDR(.text) - KERNEL_BASE) {
        *(.text* .sram.text)
    }
    _text_end = .;

    /* Start of rodata section */
    . = ALIGN(PAGE_SIZE);
    _rodata_start = .;

    .rodata : AT(ADDR(.rodata) - KERNEL_BASE) {
        *(.rodata*)
    }
    .srodata : AT(ADDR(.srodata) - KERNEL_BASE) {
        *(.srodata*)
    }
    _rodata_end = .;

    . = ALIGN(PAGE_SIZE);
    _data_start = .;
    .data : AT(ADDR(.data) - KERNEL_BASE) {
        *(.data .data.*)
    }
    .sdata : AT(ADDR(.sdata) - KERNEL_BASE) {
        __global_pointer$ = . + 0x800;
        *(.sdata*)
    }
    _data_end = .;

    /* Start of bss section */
    . = ALIGN(PAGE_SIZE);
    _bss_start = .;
    .bss : AT(ADDR(.bss) - KERNEL_BASE) {
        /* boot stack [aligned] */
        _boot_stack = .;
        . += CONFIG_STACK_SIZE;
        _boot_stack_top = .;
        /* swapper pgdir [aligned] */
        _swapper_pgd = .;
        . += PAGE_SIZE;
        _swapper_pgd_end = .;
        /* swapper tables [aligned] */
        _swapper_tables = .;
        . += PAGE_SIZE * (CONFIG_MMU_MAX_LEVEL - 1);
        _swapper_tables_end = .;
        /* periph tables_start [aligned] */
        _periph_tables_start = .;
        . += PAGE_SIZE * (CONFIG_MMU_MAX_LEVEL - 1);
        _periph_tables_end = .;
        /* boot heap [aligned] */
        _boot_heap = .;
        . += CONFIG_BOOT_HEAP_SIZE;
        _boot_heap_end = .;
        *(.bss*)
        *(COMMON)
    }
    . = ALIGN(PAGE_SIZE);
    .sbss : AT(ADDR(.sbss) - KERNEL_BASE) {
        *(.dynsbss)
        *(.sbss)
        *(.scommon)
    }
    _bss_end = .;

    . = ALIGN(PAGE_SIZE);
    _end = .;
}
