//! Authentication: `p4 login` / `p4 logout`.
//!
//! Unlike the other commands, `login` needs a secret to go *in* -- the server
//! asks for the password via a prompt, which the bridge answers from
//! [`UserInterface::set_password`]. On success the server issues a ticket
//! (stored automatically in the tickets file; see
//! [`client::Options::set_ticket_file`](crate::client::Options::set_ticket_file))
//! and reports how long it stays valid.

use crate::client::{Client, UserInterface};
use crate::commands::helpers::{from_str_value, opt_from_str, optional_string};
use crate::errors::Error;
use serde::Deserialize;
use serde::de::value::MapDeserializer;
use std::collections::HashMap;
use std::time::Duration;

/// A successful `p4 login`: the authenticated user and the lifetime of the
/// ticket just issued.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoginResult {
    /// The user the ticket was issued for.
    pub user: String,
    /// Seconds until the new ticket expires.
    pub expires_in_seconds: u64,
}

impl LoginResult {
    /// The ticket lifetime as a [`Duration`].
    pub fn expires_in(&self) -> Duration {
        Duration::from_secs(self.expires_in_seconds)
    }
}

/// Current authentication state, from `p4 login -s`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoginStatus {
    /// The user the connection is acting as.
    pub user: String,
    /// Seconds until the active ticket expires, or `None` when there is no
    /// active ticket -- i.e. the connection is not logged in.
    pub expires_in_seconds: Option<u64>,
    /// How the current command authenticated (e.g. `"HostTicket"`), when a
    /// ticket is active.
    pub authed_by: Option<String>,
}

impl LoginStatus {
    /// Whether an active ticket backs this connection.
    pub fn is_authenticated(&self) -> bool {
        self.expires_in_seconds.is_some()
    }

    /// Remaining ticket lifetime, when authenticated.
    pub fn expires_in(&self) -> Option<Duration> {
        self.expires_in_seconds.map(Duration::from_secs)
    }
}

/// Options for `p4 login`.
#[derive(Debug, Clone, Default)]
pub struct Options {
    all_hosts: bool,
    host: Option<String>,
}

impl Options {
    pub fn new() -> Options {
        Options::default()
    }

    /// `-a`: issue a ticket valid from all hosts, not just the current one.
    pub fn all_hosts(mut self) -> Options {
        self.all_hosts = true;
        self
    }

    /// `-h <host>`: issue a ticket valid from the named host (e.g. when logging
    /// in on behalf of a different machine).
    pub fn host(mut self, host: &str) -> Options {
        self.host = Some(host.to_string());
        self
    }

    fn args(&self) -> Vec<String> {
        let mut args = Vec::new();
        if self.all_hosts {
            args.push("-a".to_string());
        }
        if let Some(host) = &self.host {
            args.push("-h".to_string());
            args.push(host.clone());
        }
        args
    }
}

/// The `login` / `login -a` record: `{ User, TicketExpiration }`.
#[derive(Deserialize)]
struct LoginRecord {
    #[serde(rename = "User")]
    user: String,
    #[serde(rename = "TicketExpiration", deserialize_with = "from_str_value")]
    ticket_expiration: u64,
}

impl From<LoginRecord> for LoginResult {
    fn from(r: LoginRecord) -> Self {
        LoginResult {
            user: r.user,
            expires_in_seconds: r.ticket_expiration,
        }
    }
}

/// The `login -s` record. `TicketExpiration`/`AuthedBy` are absent when there
/// is no active ticket, so both are optional-by-absence.
#[derive(Deserialize)]
struct StatusRecord {
    #[serde(rename = "User")]
    user: String,
    #[serde(
        rename = "TicketExpiration",
        default,
        deserialize_with = "opt_from_str"
    )]
    ticket_expiration: Option<u64>,
    #[serde(rename = "AuthedBy", default, deserialize_with = "optional_string")]
    authed_by: Option<String>,
}

impl From<StatusRecord> for LoginStatus {
    fn from(r: StatusRecord) -> Self {
        LoginStatus {
            user: r.user,
            expires_in_seconds: r.ticket_expiration,
            authed_by: r.authed_by,
        }
    }
}

fn deserialize<'a, T: Deserialize<'a>>(m: HashMap<String, String>) -> Result<T, Error> {
    T::deserialize(MapDeserializer::new(m.clone().into_iter()))
        .map_err(|e| Error::SerializationError(e, m))
}

fn first_record(
    records: Vec<HashMap<String, String>>,
    what: &str,
) -> Result<HashMap<String, String>, Error> {
    records
        .into_iter()
        .next()
        .ok_or_else(|| Error::SpecError(format!("{what} returned no record")))
}

impl Client {
    /// Authenticate with `password` (`p4 login`) and return the issued ticket's
    /// user and lifetime. An incorrect password surfaces as
    /// [`Error::RawError`] ("Password invalid.").
    ///
    /// The ticket is stored in the tickets file for you, so subsequent commands
    /// on a fresh connection to the same server are authenticated automatically.
    pub fn login(&mut self, password: &str) -> Result<LoginResult, Error> {
        self.login_with(password, &Options::new())
    }

    /// [`login`](Self::login) with options (`-a` all hosts / `-h <host>`).
    pub fn login_with(&mut self, password: &str, opts: &Options) -> Result<LoginResult, Error> {
        let mut ui = UserInterface::new();
        ui.set_password(password);
        let records = self.run_records(&mut ui, "login", opts.args())?;
        deserialize::<LoginRecord>(first_record(records, "login")?).map(Into::into)
    }

    /// Report the connection's authentication state (`p4 login -s`) without
    /// needing a password. [`LoginStatus::is_authenticated`] tells you whether
    /// an active ticket backs the connection.
    ///
    /// Note: at higher server security levels a connection with no ticket makes
    /// the server *reject* `login -s` outright, which surfaces here as an
    /// [`Error::RawError`] rather than an unauthenticated [`LoginStatus`].
    pub fn login_status(&mut self) -> Result<LoginStatus, Error> {
        let mut ui = UserInterface::new();
        let records = self.run_records(&mut ui, "login", vec!["-s".to_string()])?;
        deserialize::<StatusRecord>(first_record(records, "login -s")?).map(Into::into)
    }

    /// End the current ticket (`p4 logout`).
    pub fn logout(&mut self) -> Result<(), Error> {
        let mut ui = UserInterface::new();
        self.run_records(&mut ui, "logout", Vec::new())?;
        Ok(())
    }

    /// End the user's tickets on all hosts (`p4 logout -a`).
    pub fn logout_all(&mut self) -> Result<(), Error> {
        let mut ui = UserInterface::new();
        self.run_records(&mut ui, "logout", vec!["-a".to_string()])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn login_record_deserializes() {
        // Verbatim shape of one `p4 login` record.
        let r: LoginResult = deserialize::<LoginRecord>(map(&[
            ("User", "p4rs-login"),
            ("TicketExpiration", "43200"),
        ]))
        .expect("login record")
        .into();
        assert_eq!(r.user, "p4rs-login");
        assert_eq!(r.expires_in_seconds, 43200);
        assert_eq!(r.expires_in(), Duration::from_secs(43200));
    }

    #[test]
    fn status_authenticated() {
        // Verbatim shape of `p4 login -s` with an active ticket.
        let s: LoginStatus = deserialize::<StatusRecord>(map(&[
            ("AuthedBy", "HostTicket"),
            ("TicketExpiration", "43200"),
            ("User", "p4rs-login"),
        ]))
        .expect("status record")
        .into();
        assert!(s.is_authenticated());
        assert_eq!(s.expires_in_seconds, Some(43200));
        assert_eq!(s.authed_by.as_deref(), Some("HostTicket"));
        assert_eq!(s.expires_in(), Some(Duration::from_secs(43200)));
    }

    #[test]
    fn status_unauthenticated_has_no_expiration() {
        // `login -s` with no ticket reports only the user.
        let s: LoginStatus = deserialize::<StatusRecord>(map(&[("User", "p4rs-noauth")]))
            .expect("status record")
            .into();
        assert!(!s.is_authenticated());
        assert_eq!(s.expires_in_seconds, None);
        assert_eq!(s.authed_by, None);
        assert_eq!(s.expires_in(), None);
    }

    #[test]
    fn options_build_args() {
        assert!(Options::new().args().is_empty());
        assert_eq!(Options::new().all_hosts().args(), vec!["-a"]);
        assert_eq!(
            Options::new().host("build-box").args(),
            vec!["-h", "build-box"]
        );
    }
}
