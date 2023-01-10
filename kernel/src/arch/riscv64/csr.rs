/*
 * Copyright (c) 2022 Shi Lei
 *
 * Use of this source code is governed by a MIT-style license
 * that can be found in the LICENSE file or
 * at https://opensource.org/licenses/MIT
 */

/* Status register flags */
pub const SR_SIE: usize = 0x00000002;   /* Supervisor Interrupt Enable */

pub const SR_IE: usize = SR_SIE;
