#!/usr/bin/env rfust
//! [dependencies]
//! serde_json = "1.0"
//! [dependencies.reqwest]
//! version = "0.13.2"
//! default-features = false   // disable native TLS (OpenSSL), use rustls instead
//! features = ["blocking", "json", "rustls"]

fn main() {
    println!("Making request to GitHub API...");

    // Send a blocking GET request to the GitHub API (repo information)
    let body: serde_json::Value = reqwest::blocking::Client::new()
        .get("https://api.github.com/repos/rust-lang/rust")
        .header("User-Agent", "rfust-example")   // GitHub API requires a User-Agent header
        .send()
        .expect("Request failed")
        .json()
        .expect("Failed to parse JSON response");

    // Extract and print relevant fields from the JSON response
    println!("Repository: {}", body["full_name"].as_str().unwrap_or("?"));
    println!("Stars:      {}", body["stargazers_count"]);
    println!("Language:   {}", body["language"].as_str().unwrap_or("?"));
}