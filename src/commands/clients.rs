//! Typed `p4 clients` -- list the client workspaces known to the server.

use crate::client::{Client, UserInterface};
use crate::commands::client::ClientOptions;
use crate::commands::helpers::{from_str_value, optional_string};
use crate::errors::Error;
use serde::Deserialize;

impl Client {
    /// List the server's client workspaces (`p4 clients`), typed. This is the
    /// list form: each record is a workspace summary (no `View`/`Type`), which
    /// is why the record type is [`ClientSummary`] rather than the spec's
    /// `ClientSpec`.
    pub fn clients(&mut self, options: &Options) -> Result<Vec<ClientSummary>, Error> {
        let mut ui = UserInterface::new();
        let records = self.run_records(&mut ui, "clients", options.get_args())?;
        records
            .into_iter()
            .map(|m| {
                ClientSummary::deserialize(serde::de::value::MapDeserializer::new(
                    m.clone().into_iter(),
                ))
                .map_err(|e| Error::SerializationError(e, m))
            })
            .collect()
    }
}

/// Options for `p4 clients`.
#[derive(Debug, Default)]
pub struct Options {
    max: Option<u32>,
    user: Option<String>,
    name_filter: Option<String>,
}

impl Options {
    pub fn new() -> Options {
        Options::default()
    }

    /// `-m max`: list only the first `max` client workspaces.
    pub fn max(mut self, max: u32) -> Self {
        self.max = Some(max);
        self
    }

    /// `-u user`: list only clients owned by `user`.
    pub fn user(mut self, user: &str) -> Self {
        self.user = Some(user.to_string());
        self
    }

    /// `-E name_filter`: list only clients whose name matches the filter,
    /// case-insensitively.
    pub fn name_filter(mut self, filter: &str) -> Self {
        self.name_filter = Some(filter.to_string());
        self
    }

    pub fn get_args(&self) -> Vec<String> {
        let mut args = Vec::new();
        if let Some(max) = self.max {
            args.push("-m".to_string());
            args.push(max.to_string());
        }
        if let Some(user) = &self.user {
            args.push("-u".to_string());
            args.push(user.clone());
        }
        if let Some(filter) = &self.name_filter {
            args.push("-E".to_string());
            args.push(filter.clone());
        }
        args
    }
}

/// One client workspace as reported by `p4 clients`. This is the list summary,
/// deliberately distinct from `ClientSpec`: `clients` never reports the view or
/// the client type, and (a real p4 quirk) it names the workspace under a
/// lowercase `client` key while the rest stay capitalized. In tagged output
/// `Update`/`Access` are epoch seconds.
#[derive(Debug, Clone, Deserialize)]
pub struct ClientSummary {
    /// Note the lowercase key -- unlike `ClientSpec`'s `Client`.
    #[serde(rename = "client")]
    pub client: String,

    #[serde(rename = "Owner", default, deserialize_with = "optional_string")]
    pub owner: Option<String>,

    /// May be empty (unbound to a specific host).
    #[serde(rename = "Host", default, deserialize_with = "optional_string")]
    pub host: Option<String>,

    #[serde(rename = "Description", default)]
    pub description: String,

    #[serde(rename = "Root")]
    pub root: String,

    #[serde(rename = "Options", deserialize_with = "from_str_value")]
    pub options: ClientOptions,

    #[serde(
        rename = "SubmitOptions",
        default,
        deserialize_with = "optional_string"
    )]
    pub submit_options: Option<String>,

    #[serde(rename = "LineEnd", default, deserialize_with = "optional_string")]
    pub line_end: Option<String>,

    /// Last spec update, seconds since the Unix epoch.
    #[serde(rename = "Update", deserialize_with = "from_str_value")]
    pub update: u64,

    /// Last access, seconds since the Unix epoch.
    #[serde(rename = "Access", deserialize_with = "from_str_value")]
    pub access: u64,

    /// Present only for a client bound to a stream.
    #[serde(rename = "Stream", default, deserialize_with = "optional_string")]
    pub stream: Option<String>,

    /// Present only in a distributed/edge configuration.
    #[serde(rename = "ServerID", default, deserialize_with = "optional_string")]
    pub server_id: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::de::value::MapDeserializer;
    use std::collections::HashMap;

    /// Field shapes captured VERBATIM from a real `p4 clients` tagged record
    /// (p4d 2025.2). Note the lowercase `client` key -- a genuine p4 quirk --
    /// and the epoch-seconds Update/Access.
    fn captured_record() -> HashMap<String, String> {
        [
            ("client", "cls-a"),
            ("Owner", "danrs"),
            ("Host", "Dan-Desktop"),
            ("Description", "p4-rs test client cls-a\n"),
            (
                "Root",
                "C:\\Users\\danrs\\AppData\\Local\\Temp\\p4-rs-it-clients-capture-25772-1\\a",
            ),
            (
                "Options",
                "noallwrite noclobber nocompress unlocked nomodtime normdir",
            ),
            ("SubmitOptions", "submitunchanged"),
            ("LineEnd", "local"),
            ("Update", "1784465081"),
            ("Access", "1784465081"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
    }

    #[test]
    fn client_summary_deserializes_from_captured_record() {
        let m = captured_record();
        let w = ClientSummary::deserialize(MapDeserializer::<_, serde::de::value::Error>::new(
            m.into_iter(),
        ))
        .expect("deserialize captured clients record");

        assert_eq!(w.client, "cls-a");
        assert_eq!(w.owner.as_deref(), Some("danrs"));
        assert_eq!(w.host.as_deref(), Some("Dan-Desktop"));
        assert_eq!(w.description, "p4-rs test client cls-a\n");
        assert_eq!(w.submit_options.as_deref(), Some("submitunchanged"));
        assert_eq!(w.line_end.as_deref(), Some("local"));
        assert_eq!(w.update, 1_784_465_081);
        assert_eq!(w.access, 1_784_465_081);
        // Options parsed via the reused ClientOptions FromStr.
        assert!(!w.options.allwrite);
        assert!(!w.options.locked);
        assert!(w.options.unknown.is_empty());
        // Absent conditionals default to None.
        assert!(w.stream.is_none());
        assert!(w.server_id.is_none());
    }

    #[test]
    fn stream_and_server_id_are_captured_when_present() {
        let mut m = captured_record();
        m.insert("Stream".to_string(), "//streams/main".to_string());
        m.insert("ServerID".to_string(), "edge-1".to_string());

        let w = ClientSummary::deserialize(MapDeserializer::<_, serde::de::value::Error>::new(
            m.into_iter(),
        ))
        .expect("deserialize record with stream/serverid");
        assert_eq!(w.stream.as_deref(), Some("//streams/main"));
        assert_eq!(w.server_id.as_deref(), Some("edge-1"));
    }

    #[test]
    fn options_build_expected_args() {
        assert!(Options::new().get_args().is_empty());
        assert_eq!(
            Options::new()
                .max(5)
                .user("danrs")
                .name_filter("cls-*")
                .get_args(),
            vec!["-m", "5", "-u", "danrs", "-E", "cls-*"]
        );
    }
}
