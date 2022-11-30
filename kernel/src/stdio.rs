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

    pub fn put_u64(&mut self, n: u64) {
        for i in 1..=16 {
            let mut c = ((n >> ((16 - i)*4)) & 0xF) as u8;
            if c >= 10 {
                c -= 10;
                c += 'A' as u8;
            } else {
                c += '0' as u8;
            }
            sbi::console_putchar(c as char);
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
