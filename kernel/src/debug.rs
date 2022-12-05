/*
 * Copyright (c) 2022 Shi Lei
 *
 * Use of this source code is governed by a MIT-style license
 * that can be found in the LICENSE file or
 * at https://opensource.org/licenses/MIT
 */

#![allow(dead_code)]

/* debug print levels */
pub const CRITICAL  : u32 = 0;
pub const ALWAYS    : u32 = 0;
pub const INFO      : u32 = 1;
pub const SPEW      : u32 = 2;

pub const DEBUG_PRINT_LEVEL: u32 = SPEW;

#[macro_export]
macro_rules! dprintf {
    ($level: expr, $($arg:tt)*) => (
        if $level <= DEBUG_PRINT_LEVEL {
            print!($($arg)*);
        }
    );
}

/************************************/

/*
use spin::Mutex;
use core::fmt;
use crate::arch::sbi;
use core::fmt::Write;

#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => ({
        $crate::stdio::_print(format_args!($($arg)*));
    });
}

#[macro_export]
macro_rules! println {
    () => ($crate::print!("\n"));
    ($($arg:tt)*) => ($crate::print!("{}\n", format_args!($($arg)*)));
}

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

pub fn _print(args: fmt::Arguments) {
    STDOUT.lock().write_fmt(args).unwrap();
}
*/
