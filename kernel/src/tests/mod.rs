/*
 * Copyright (c) 2022 Shi Lei
 *
 * Use of this source code is governed by a MIT-style license
 * that can be found in the LICENSE file or
 * at https://opensource.org/licenses/MIT
 */

use cmpct::test_cmpct;
use heap::test_heap;
use mutex::test_mutex;

mod cmpct;
mod heap;
mod mutex;

#[cfg(feature = "unittest")]
pub fn do_tests() {
    println!("\n[TESTS: start ...]\n");
    test_cmpct();
    test_heap();
    test_mutex();
    println!("\n[TESTS: finished!]\n");
}
