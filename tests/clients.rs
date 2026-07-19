//! Integration tests for typed `p4 clients` (list client workspaces).
//!
//! Server-backed tests are `#[ignore]`d; run them with:
//!
//! ```text
//! P4D_BIN=/path/to/p4d cargo test --test clients -- --ignored
//! ```

mod common;

use common::{TestServer, create_client, dump_records, skip};
use p4_rs::client;
use p4_rs::commands::clients;

/// Capture-first harness: dump real `clients` tagged records so the typed
/// structs can be shaped against reality. Run manually with:
///
/// ```text
/// P4D_BIN=... P4RS_CAPTURE=1 cargo test --test clients -- --ignored --nocapture
/// ```
#[test]
#[ignore = "capture harness; run manually with P4D_BIN and P4RS_CAPTURE=1 set"]
fn capture_clients_records() {
    if std::env::var("P4RS_CAPTURE").is_err() {
        eprintln!("set P4RS_CAPTURE=1 to run the capture harness");
        return;
    }
    let Some(server) = TestServer::start("clients-capture") else {
        eprintln!("P4D_BIN not set");
        return;
    };

    let mut c = server.connect();
    create_client(&mut c, "cls-a", &server.root.join("a"));

    let mut ui = client::UserInterface::new();
    let r = c.run_records(&mut ui, "clients", Vec::new());
    dump_records("clients", &r);
}

/// clients() lists the workspaces on the server, typed.
#[test]
#[ignore = "requires P4D_BIN; run with `cargo test -- --ignored`"]
fn clients_lists_created_workspaces() {
    let Some(server) = TestServer::start("clients") else {
        skip("clients_lists_created_workspaces");
        return;
    };

    let mut c = server.connect();
    create_client(&mut c, "cls-a", &server.root.join("a"));
    create_client(&mut c, "cls-b", &server.root.join("b"));

    let listed = c
        .clients(&clients::Options::new())
        .expect("list clients against a live p4d");

    let a = listed
        .iter()
        .find(|w| w.client == "cls-a")
        .expect("cls-a should be listed");
    let b = listed
        .iter()
        .find(|w| w.client == "cls-b")
        .expect("cls-b should be listed");

    // Update is a real epoch stamp on a saved client.
    assert!(a.update > 0, "tagged Update should be an epoch timestamp");
    assert!(b.update > 0, "tagged Update should be an epoch timestamp");

    // Options parsed: a default client is not allwrite.
    assert!(!a.options.allwrite, "default client should not be allwrite");
}
