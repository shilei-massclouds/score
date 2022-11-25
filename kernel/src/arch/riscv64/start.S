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

    call lk_main
