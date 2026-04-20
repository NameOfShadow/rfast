#!/usr/bin/env rfust
//! [dependencies]
//! colored = "3.1.1"      // for terminal coloring
//! chrono = "0.4"       // for date/time handling

use colored::*;          // import the Colorize trait
use chrono::Local;       // import local timezone

fn main() {
    let now = Local::now();   // get the current local date and time

    // Print current time in bold cyan
    println!(
        "{} {}",
        "Current time:".bold(),
        now.format("%H:%M:%S").to_string().cyan().bold()
    );

    // Print current date in bold yellow
    println!(
        "{} {}",
        "Date:         ".bold(),
        now.format("%d.%m.%Y").to_string().yellow()
    );

    // Collect command‑line arguments passed to the script (skip the script name itself)
    let args: Vec<String> = std::env::args().skip(1).collect();
    if !args.is_empty() {
        // Print the arguments in green, joined by commas
        println!(
            "{} {}",
            "Arguments:    ".bold(),
            args.join(", ").green()
        );
    }
}