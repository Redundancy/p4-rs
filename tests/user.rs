//! Integration tests for the typed `user` spec command (`user -o` / `user -i`).
//!
//! Server-backed tests are `#[ignore]`d; run them with:
//!
//! ```text
//! P4D_BIN=/path/to/p4d cargo test --test user -- --ignored
//! ```

mod common;

use common::{TestServer, dump_records, skip};
use p4_rs::client;

/// Capture-first workflow: dump the real `user -o` record before and after a
/// save, so the typed struct is shaped against reality (and so we can confirm
/// Update/Access appear only once the record exists). Manual tool, gated on
/// P4RS_CAPTURE; run with:
///
/// ```text
/// P4D_BIN=... P4RS_CAPTURE=1 cargo test --test user -- --ignored --nocapture
/// ```
#[test]
#[ignore = "capture harness; run manually with P4D_BIN and P4RS_CAPTURE=1 set"]
fn capture_user_records() {
    if std::env::var("P4RS_CAPTURE").is_err() {
        eprintln!("set P4RS_CAPTURE=1 to run the capture harness");
        return;
    }
    let Some(server) = TestServer::start("user-capture") else {
        eprintln!("P4D_BIN not set");
        return;
    };

    let mut c = server.connect();
    let mut ui = client::UserInterface::new();

    // Before: a defaulted template for a user that doesn't exist yet.
    let before = c.run_records(&mut ui, "user", vec!["-o".into()]);
    dump_records("user -o (before save)", &before);

    // Resolve our own user name from the template and create the record.
    let me = before
        .as_ref()
        .ok()
        .and_then(|r| r.first())
        .and_then(|r| r.get("User"))
        .expect("template User field")
        .clone();
    ui.set_input(&format!(
        "User:\t{me}\n\nEmail:\t{me}@spec.test\n\nFullName:\tCapture Dump\n"
    ));
    c.run_records(&mut ui, "user", vec!["-i".into()])
        .expect("save own user spec");

    // After: the saved record now carries server-managed Update/Access.
    let after = c.run_records(&mut ui, "user", vec!["-o".into()]);
    dump_records("user -o (after save)", &after);
}

/// The read -> modify -> save -> re-read cycle for a user spec, all typed.
#[test]
#[ignore = "requires P4D_BIN; run with `cargo test -- --ignored`"]
fn user_spec_create_modify_reread() {
    let Some(server) = TestServer::start("user-spec") else {
        skip("user_spec_create_modify_reread");
        return;
    };

    let mut c = server.connect();

    // The connection's own user (`user -o` with no name). The authoritative
    // name is the template's own `User` field -- info()'s userName is
    // "*unknown*" until the record exists. Note: unlike a client spec, the user
    // template already carries formatted Update/Access dates even before it is
    // saved (observed on p4d 2022.2), so those are not a create-vs-template
    // signal; the email/full-name round trip below is what proves the save.
    let mut spec = c.user_spec(None).expect("read own user template");
    let me = spec.user.clone();
    assert!(!me.is_empty(), "template must carry a user name");

    // Modify: set email + full name to distinctive values, then save.
    spec.email = format!("{me}@spec.test");
    spec.full_name = "Spec Roundtrip".to_string();
    c.save_user_spec(&spec).expect("save modified user spec");

    // Re-read: our modifications persisted and the server stamped it.
    let saved = c.user_spec(None).expect("re-read saved user spec");
    assert_eq!(saved.email, format!("{me}@spec.test"));
    assert_eq!(saved.full_name, "Spec Roundtrip");
    assert!(
        saved.update.is_some(),
        "saved user spec carries an Update stamp"
    );
}
