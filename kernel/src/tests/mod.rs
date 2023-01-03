/*
 * Copyright (c) 2022 Shi Lei
 *
 * Use of this source code is governed by a MIT-style license
 * that can be found in the LICENSE file or
 * at https://opensource.org/licenses/MIT
 */

use cmpct::test_cmpct;
use heap::test_heap;

mod cmpct;
mod heap;

#[cfg(feature = "unittest")]
pub fn do_tests() {
    println!("\n[TESTS: start ...]\n");
    test_cmpct();
    test_heap();
    println!("\n[TESTS: finished!]\n");
}