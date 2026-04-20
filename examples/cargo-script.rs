#!/usr/bin/env run-cargo-script
// cargo-deps: time="0.1.25", colored="2.0"

extern crate time;
use colored::*;

fn main() {
    println!("{}", "Hello from cargo-script short format!".green());
    println!("Current time: {}", time::now().rfc822z());
}