//# serde_json = "*"
//# anyhow = "1.0"

use serde_json::json;

fn main() {
    println!("{}", json!({"ok": true}));
}