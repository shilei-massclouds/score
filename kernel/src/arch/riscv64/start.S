/*
 * Copyright (c) 2022 Shi Lei
 *
 * Use of this source code is governed by a MIT-style license
 * that can be found in the LICENSE file or
 * at https://opensource.org/licenses/MIT
 */

.section ".text.entry", "ax"
.globl _start
_start:
    la sp, _boot_stack_top

    la t0, 2f
    csrw stvec, t0

    call lk_main

    /* Loop forever for debug. */
    .align 2
 2: wfi
    j 2b
