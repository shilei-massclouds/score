/*
 * Copyright (c) 2022 Shi Lei
 *
 * Use of this source code is governed by a MIT-style license
 * that can be found in the LICENSE file or
 * at https://opensource.org/licenses/MIT
 */

use spin::Mutex;
use core::fmt;
use crate::arch::sbi;

pub struct StdOut;

impl StdOut {
    pub fn puts(&mut self, s: &str) {
        for c in s.chars() {
            sbi::console_putchar(c);
        }
    }
}

impl fmt::Write for StdOut {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.puts(s);
        Ok(())
    }
}

pub static STDOUT: Mutex<StdOut> = Mutex::new(StdOut);
