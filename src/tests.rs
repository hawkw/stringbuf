use super::{Reader, Writer};
use std::fmt::Write;

#[test]
fn basic() {
    loom::model(|| {
        let (tx, mut rx) = super::with_capacity(10);
        let tx2 = tx.clone();
        let t1 = loom::thread::spawn(move || {
            tx.write(|s| write!(s, "hello"));
            tx.write(|s| write!(s, "world"));
        });
        let t2 = loom::thread::spawn(move || {
            tx2.write(|s| write!(s, "have lots"));
            tx2.write(|s| write!(s, "of fun"));
        });
        let mut written = Vec::new();
        while let Ok(read) = rx.read(String::clone) {
            println!("read: {:?}", read);
            written.push(read);
        }
        println!("channel closed!");
        t1.join();
        t2.join();
        assert!(written.iter().any(|s| s == "hello"));
        assert!(written.iter().any(|s| s == "world"));

        assert!(written.iter().any(|s| s == "have lots"));
        assert!(written.iter().any(|s| s == "of fun"));
    })
}
