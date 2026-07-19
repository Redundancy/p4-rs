//! Integration + capture tests for the typed `group` / `groups` commands.
//!
//! The server-backed tests are `#[ignore]`d; run them with:
//!
//! ```text
//! P4D_BIN=/path/to/p4d cargo test --test group -- --ignored
//! ```

mod common;

use common::{TestServer, dump_records, skip};
use p4_rs::client;
use p4_rs::commands::group::{GroupSpec, Limit};

/// Capture harness: dump the real tagged records so the typed structs can be
/// shaped against reality. Run manually:
///
/// ```text
/// P4RS_CAPTURE=1 cargo test --test group -- --ignored --nocapture
/// ```
#[test]
#[ignore = "capture harness; run manually with P4D_BIN and P4RS_CAPTURE=1 set"]
fn capture_group_records() {
    if std::env::var("P4RS_CAPTURE").is_err() {
        eprintln!("set P4RS_CAPTURE=1 to run the capture harness");
        return;
    }
    let Some(server) = TestServer::start("group-capture") else {
        eprintln!("P4D_BIN not set");
        return;
    };

    let mut c = server.connect();
    let mut ui = client::UserInterface::new();

    // Template for a group that doesn't exist yet.
    let r = c.run_records(&mut ui, "group", vec!["-o".into(), "newgrp".into()]);
    dump_records("group -o newgrp (template)", &r);

    // Find who we are (see integration test note on info() reporting *unknown*).
    let template = c
        .run_records(&mut ui, "user", vec!["-o".into()])
        .expect("user -o template");
    let me = template
        .first()
        .and_then(|r| r.get("User"))
        .expect("template User field")
        .clone();

    // Save a group with one user, then dump the list and the re-read spec.
    ui.set_input(&format!(
        "Group:\tnewgrp\n\nTimeout:\t43200\n\nUsers:\n\t{me}\n"
    ));
    c.run_records(&mut ui, "group", vec!["-i".into()])
        .expect("group -i");

    let r = c.run_records(&mut ui, "groups", Vec::new());
    dump_records("groups", &r);

    let r = c.run_records(&mut ui, "group", vec!["-o".into(), "newgrp".into()]);
    dump_records("group -o newgrp (saved)", &r);
}

/// The create -> modify -> save -> list -> re-read cycle for group specs, typed.
#[test]
#[ignore = "requires P4D_BIN; run with `cargo test -- --ignored`"]
fn group_create_modify_reread() {
    let Some(server) = TestServer::start("group") else {
        skip("group_create_modify_reread");
        return;
    };

    let mut c = server.connect();

    // A spec for a group that doesn't exist yet is a defaulted template.
    let mut spec = c.group_spec("devs").expect("read group template");
    assert_eq!(spec.group, "devs");
    assert!(spec.users.is_empty(), "template group has no users");

    // info()'s userName is "*unknown*" until our user record exists, so read
    // the connection's user name from the `user -o` template instead.
    let mut ui = client::UserInterface::new();
    let template = c
        .run_records(&mut ui, "user", vec!["-o".to_string()])
        .expect("user -o template");
    let me = template
        .first()
        .and_then(|r| r.get("User"))
        .expect("template User field")
        .clone();

    // Modify: add ourselves as a member and set a numeric MaxResults. A group
    // needs at least one user/owner/subgroup for the server to create it.
    spec.users.push(me.clone());
    spec.max_results = Limit::Value(1000);

    c.save_group_spec(&spec).expect("save group spec");

    // groups() lists the newly created group.
    let listed = c.groups().expect("list groups");
    assert!(
        listed.iter().any(|g| g.group == "devs"),
        "created group should be listed, got {listed:?}"
    );

    // Re-read: our modifications round-tripped.
    let saved: GroupSpec = c.group_spec("devs").expect("re-read saved group");
    assert!(
        saved.users.contains(&me),
        "member should have round-tripped, got {:?}",
        saved.users
    );
    assert_eq!(saved.max_results, Limit::Value(1000));
}
