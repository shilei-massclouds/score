/*
 * Copyright (c) 2022 Shi Lei
 *
 * Use of this source code is governed by a MIT-style license
 * that can be found in the LICENSE file or
 * at https://opensource.org/licenses/MIT
 */

    /*
     * Image header expected by Linux boot-loaders.
     */

    .section ".text.entry", "ax"
    .balign 4
    .globl _start
_start:
    /* jump to start kernel */
    j _start_kernel

_hart_lottery:
    .word 0

    /*
     * Entry
     */

    .balign 4
    .globl _start_kernel
_start_kernel:
    /* Mask all interrupts */
    csrw sie, zero
    csrw sip, zero

    /* Load the global pointer */
    .option push
    .option norelax
    la gp, __global_pointer$
    .option pop

    /*
     * Disable FPU to detect illegal usage of
     * floating point in kernel space
     */
    li t0, 0x00006000   /* SR_FS */
    csrc sstatus, t0

    /* Pick one hart to run the main boot sequence */
    la a3, _hart_lottery
    li a2, 1
    amoadd.w a3, a2, (a3)
    bnez a3, _secondary_start

    /* Clear BSS for flat non-ELF images */
    la a3, __bss_start
    la a4, __bss_stop
    ble a4, a3, clear_bss_done
clear_bss:
    sd zero, (a3)
    add a3, a3, 8   /* RISCV_SZPTR */
    blt a3, a4, clear_bss
clear_bss_done:

    /* Save hart ID and DTB physical address */
    mv s0, a0
    mv s1, a1

    /*
    la a2, boot_cpu_hartid
    sd a0, (a2)
    */

    la sp, _boot_stack_top

    /* Initialize page tables */
    call setup_vm

    /* Initialize page tables and relocate to virtual addresses */

    la t0, _park
    csrw stvec, t0

    tail lk_main

    .balign 4
_secondary_start:
    j _secondary_start

    /* Loop forever for debug. */
    .balign 4
_park: wfi
    j _park
