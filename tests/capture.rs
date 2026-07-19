//! Scratch capture harness: dump real tagged records from a live p4d so typed
//! structs can be shaped against reality. A manual tool, not a test -- run
//! explicitly with:
//!
//! ```text
//! P4D_BIN=... P4RS_CAPTURE=1 cargo test --test capture -- --ignored --nocapture
//! ```
//!
//! When adding a NEW command, do not extend this file: put a capture-gated dump
//! test in that command's own tests/<name>.rs instead (using
//! common::dump_records), so parallel work never collides here.

mod common;

use common::{TestServer, dump_records};
use p4_rs::client;

#[test]
#[ignore = "capture harness; run manually with P4D_BIN and P4RS_CAPTURE=1 set"]
fn capture_records() {
    if std::env::var("P4RS_CAPTURE").is_err() {
        eprintln!("set P4RS_CAPTURE=1 to run the capture harness");
        return;
    }
    let Some(server) = TestServer::start("capture") else {
        eprintln!("P4D_BIN not set");
        return;
    };

    let mut c = server.connect_with_client("cap-ws");
    let mut ui = client::UserInterface::new();

    let r = c.run_records(&mut ui, "user", vec!["-o".into()]);
    dump_records("user -o", &r);

    let r = c.run_records(&mut ui, "users", Vec::new());
    dump_records("users", &r);

    let r = c.run_records(&mut ui, "client", vec!["-o".into(), "cap-ws".into()]);
    dump_records("client -o (template)", &r);

    let r = c.run_records(&mut ui, "info", Vec::new());
    dump_records("info", &r);
}
