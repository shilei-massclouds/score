/*
 * Copyright (c) 2022 Shi Lei
 *
 * Use of this source code is governed by a MIT-style license
 * that can be found in the LICENSE file or
 * at https://opensource.org/licenses/MIT
 */

/* Todo: define these symbols in a generated file and include it */
CONFIG_NR_CPUS = 8;
RISCV_SZPTR = 8;
PAGE_SHIFT = 12;
SR_FS = 0x00006000;
KERNEL_BASE = 0xffffffff00000000;

    /*
     * Image header expected by Linux boot-loaders.
     */

    .section ".text.entry", "ax"
    .balign 4
    .globl _start
_start:
    /* jump to start kernel */
    j _start_kernel

    .balign 4
    .global relocate_enable_mmu
relocate_enable_mmu:
    /* Relocate return address */
    li a1, KERNEL_BASE
    la a2, _start
    sub a1, a1, a2
    add ra, ra, a1

    /* Point stvec to virtual address of intruction after satp write */
    la a2, 1f
    add a2, a2, a1
    csrw stvec, a2

    /* Compute satp for kernel page tables */
    la a0, _swapper_pgd
    srl a2, a0, PAGE_SHIFT
    la a1, _satp_mode
    ld a1, 0(a1)
    or a2, a2, a1

    /*
     * Load kernel page directory, which will cause us to trap to
     * stvec if VA != PA, or simply fall through if VA == PA.  We need a
     * full fence here because setup_vm() just wrote these PTEs and we need
     * to ensure the new translations are in use.
     */
    sfence.vma
    csrw satp, a2
    .balign 4
1:
    /* Set a full fence again */
    sfence.vma

    /* Set trap vector to spin forever to help debug */
    la a0, .Lpark
    csrw stvec, a0

    /* Reload the global pointer */
    .option push
    .option norelax
    la gp, __global_pointer$
    .option pop

    ret

    /*
     * Entry data area:
     */

    .balign 8
_hart_lottery:  /* Lottery for electing boot hart */
    .dword 0
_satp_mode:
    .dword 0
_boot_cpu_hartid:
    .dword 0
_dtb_pa:
    .dword 0

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
    li t0, SR_FS
    csrc sstatus, t0

    li t0, CONFIG_NR_CPUS
    blt a0, t0, .Lgood_cores
    tail .Lpark
.Lgood_cores:

    /* Pick one hart to run the main boot sequence */
    la a3, _hart_lottery
    li a2, 1
    amoadd.d a3, a2, (a3)
    bnez a3, .Lsecondary_start

    /* Clear BSS for flat non-ELF images */
    la a3, __bss_start
    la a4, __bss_stop
    ble a4, a3, clear_bss_done
clear_bss:
    sd zero, (a3)
    add a3, a3, RISCV_SZPTR
    blt a3, a4, clear_bss
clear_bss_done:

    /* Save hart ID and DTB physical address */
    la a2, _boot_cpu_hartid
    sd a0, (a2)
    la a2, _dtb_pa
    sd a1, (a2)

    /* Initialize boot stack */
    la sp, _boot_stack_top

    /* Initialize page tables */
    call setup_vm

    /* Relocate to virtual addresses */
    call relocate_enable_mmu

    //call setup_trap_vector
    la t0, .Lpark
    csrw stvec, t0

    /* Restore C environment */
    la sp, _boot_stack_top

    tail lk_main

    .balign 4
.Lsecondary_start:
    j .Lsecondary_start

    /* Loop forever for debug. */
    .balign 4
.Lpark: wfi
    j .Lpark
