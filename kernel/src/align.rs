/*
 * Copyright (c) 2022 Shi Lei
 *
 * Use of this source code is governed by a MIT-style license
 * that can be found in the LICENSE file or
 * at https://opensource.org/licenses/MIT
 */

#[macro_export]
macro_rules! ROUNDUP {
    ($a: expr, $b: expr) => {((($a) + (($b)-1)) & !(($b)-1))}
}

#[macro_export]
macro_rules! ROUNDDOWN {
    ($a: expr, $b: expr) => {(($a) & !(($b)-1))}
}

#[macro_export]
macro_rules! ALIGN {
    ($a: expr, $b: expr) => {ROUNDUP!($a, $b)}
}

#[macro_export]
macro_rules! IS_ALIGNED {
    ($a: expr, $b: expr) => {((($a) & (($b) - 1)) == 0)}
}

#[macro_export]
macro_rules! PAGE_ALIGN {
    ($a: expr) => {ALIGN!($a, PAGE_SIZE)}
}

#[macro_export]
macro_rules! ROUNDUP_PAGE_SIZE {
    ($x: expr) => {ROUNDUP!($x, PAGE_SIZE)}
}

#[macro_export]
macro_rules! IS_PAGE_ALIGNED {
    ($x: expr) => {IS_ALIGNED!($x, PAGE_SIZE)}
}
