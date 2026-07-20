//! Integration tests for the authentication commands (`login` / `logout`)
//! against a real `p4d`. `#[ignore]`d; run with:
//!
//! ```text
//! P4D_BIN=/path/to/p4d cargo test --test login -- --ignored
//! ```
//!
//! The bridge feeds the password via the Prompt strand and each connection uses
//! a tickets file scoped to the server root (see `common::connect_as`), so these
//! never touch the developer's shared tickets file.

mod common;

use common::{TestServer, set_password, skip};

const USER: &str = "p4rs-login";
const PASSWORD: &str = "p4rs-secret-123";

/// login issues a ticket, and login -s then reports the connection as
/// authenticated with a matching expiration.
#[test]
#[ignore = "requires P4D_BIN; run with `cargo test --test login -- --ignored`"]
fn login_issues_ticket_and_status_reflects_it() {
    let Some(server) = TestServer::start("login") else {
        skip("login_issues_ticket_and_status_reflects_it");
        return;
    };

    let mut c = server.connect_as(USER);
    set_password(&mut c, USER, PASSWORD);

    let result = c.login(PASSWORD).expect("login");
    assert_eq!(result.user, USER);
    assert!(
        result.expires_in_seconds > 0,
        "ticket should have a positive lifetime, got {}",
        result.expires_in_seconds
    );

    let status = c.login_status().expect("login -s");
    assert!(status.is_authenticated(), "status should be authenticated");
    assert_eq!(status.user, USER);
    assert!(status.expires_in_seconds.unwrap() > 0);
    assert!(
        status.authed_by.is_some(),
        "an active ticket reports how it authenticated"
    );
}

/// An incorrect password is rejected as a typed error, not a bogus ticket.
#[test]
#[ignore = "requires P4D_BIN; run with `cargo test --test login -- --ignored`"]
fn wrong_password_is_rejected() {
    let Some(server) = TestServer::start("login-bad") else {
        skip("wrong_password_is_rejected");
        return;
    };

    let mut c = server.connect_as(USER);
    set_password(&mut c, USER, PASSWORD);

    let err = c
        .login("not-the-password")
        .expect_err("wrong password errors");
    // The server reports "Password invalid." -- surfaced as a RawError.
    let msg = format!("{err:?}");
    assert!(
        msg.contains("Password") || msg.contains("password"),
        "unexpected error: {msg}"
    );
}

/// login persists a ticket to the (scoped) tickets file; logout removes it.
///
/// This reads the tickets file directly rather than asserting via `login -s`,
/// whose "not authenticated" signal is confounded on developer machines that
/// carry an ambient `P4PASSWD` (registry `p4 set` / `P4CONFIG`) the API falls
/// back to. The file is the deterministic record of the ticket's existence.
#[test]
#[ignore = "requires P4D_BIN; run with `cargo test --test login -- --ignored`"]
fn logout_clears_ticket() {
    let Some(server) = TestServer::start("logout") else {
        skip("logout_clears_ticket");
        return;
    };

    let ticket_file = server.ticket_file();
    let mut c = server.connect_as(USER);
    set_password(&mut c, USER, PASSWORD);

    c.login(PASSWORD).expect("login");
    let after_login = std::fs::read_to_string(&ticket_file).unwrap_or_default();
    assert!(
        after_login.contains(USER),
        "login should persist a ticket for {USER} to the tickets file, got: {after_login:?}"
    );

    c.logout().expect("logout");
    let after_logout = std::fs::read_to_string(&ticket_file).unwrap_or_default();
    assert!(
        !after_logout.contains(USER),
        "logout should remove {USER}'s ticket from the file, got: {after_logout:?}"
    );
}
