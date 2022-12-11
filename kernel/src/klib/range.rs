/*
 * Copyright (c) 2022 Shi Lei
 *
 * Use of this source code is governed by a MIT-style license
 * that can be found in the LICENSE file or
 * at https://opensource.org/licenses/MIT
 */

/* given two offset/length pairs, determine if they overlap at all */
#[inline]
pub fn intersects(offset1: usize, len1: usize, offset2: usize, len2: usize)
    -> bool {
    /* Can't overlap a zero-length region. */
    if len1 == 0 || len2 == 0 {
        return false;
    }

    if offset1 <= offset2 {
        /* doesn't intersect, 1 is completely below 2 */
        if offset1 + len1 <= offset2 {
            return false;
        }
    } else if offset1 >= offset2 + len2 {
        /* 1 is completely above 2 */
        return false;
    }

    true
}
