//! [dependencies]

fn main() {
    println!("Running main");
}

#[test]
fn it_works() {
    assert_eq!(2 + 2, 4);
}

#[test]
fn it_fails() {
    assert_eq!(2 + 2, 5);
}