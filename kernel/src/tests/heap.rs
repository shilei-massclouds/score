/*
 * Copyright (c) 2022 Shi Lei
 *
 * Use of this source code is governed by a MIT-style license
 * that can be found in the LICENSE file or
 * at https://opensource.org/licenses/MIT
 */

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

pub fn test_heap() {
    test_string();
    test_vec();
}

fn test_string() {
    println!(" Test: alloc string ...");
    {
        let str0 = String::from("Hello, ");
        assert!(str0 == "Hello, ");
        let str1 = str0 + "world!";
        assert!(str1 == "Hello, world!");
    }
    println!(" Test: alloc string ok!\n");
}

struct Test {
    a: u32,
    b: u64,
}

fn test_vec() {
    println!(" Test: alloc vec ...");
    {
        let v0 = vec!(0, 1, 2);
        for v in &v0 {
            print!("{}, ", v);
        }
        println!("len: {}", &v0.len());
    }

    {
        let mut v1 = Vec::<Test>::new();
        v1.push(Test { a: 1, b: 2 });
        for v in &v1 {
            print!("({} : {}), ", v.a, v.b);
        }
        println!("len: {}", &v1.len());
    }
    println!(" Test: alloc vec ok!\n");
}