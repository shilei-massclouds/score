/*
 * Copyright (c) 2022 Shi Lei
 *
 * Use of this source code is governed by a MIT-style license
 * that can be found in the LICENSE file or
 * at https://opensource.org/licenses/MIT
 */

use crate::locking::mutex::Mutex;

struct Test {
    a: usize,
}

impl Test {
    const fn new() -> Self {
        Self {
            a: 0,
        }
    }
}

static TEST_MUTEX: Mutex<Test> = Mutex::new(Test::new());

pub fn test_mutex() {
    println!(" Test: mutex ...");
    {
        let mut test = TEST_MUTEX.lock();
        println!("Test: a = {}; Then change it!", test.a);
        test.a = 1;
        println!("Test: Now a = {}", test.a);
    }
    println!(" Test: mutex ok!");
}
