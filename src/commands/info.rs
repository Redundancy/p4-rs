use crate::client::{Client, UserInterface};
use crate::commands::helpers::optional_string;
use crate::errors::Error;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::str::FromStr;

impl Client {
    /// Typed `p4 info`.
    pub fn info(&mut self, options: &Options) -> Result<Info, Error> {
        let mut ui = UserInterface::new();
        let mut records = self.run_records(&mut ui, "info", options.get_args())?;
        // info produces exactly one tagged record; deserializing an empty map
        // (no output) reports the missing fields through SerializationError.
        let m = if records.is_empty() {
            HashMap::new()
        } else {
            records.swap_remove(0)
        };
        Info::deserialize(serde::de::value::MapDeserializer::new(
            m.clone().into_iter(),
        ))
        .map_err(|e| Error::SerializationError(e, m))
    }
}

pub struct Options {
    short: bool,
}

impl Default for Options {
    fn default() -> Self {
        Options::new()
    }
}

impl Options {
    pub fn new() -> Options {
        Options { short: false }
    }

    pub fn shortened(mut self) -> Self {
        self.short = true;
        self
    }

    pub fn get_args(&self) -> Vec<String> {
        let mut args = Vec::new();
        if self.short {
            args.push("-s".to_string());
        }
        args
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub enum CaseHandling {
    #[serde(rename = "sensitive")]
    Sensitive,
    #[serde(rename = "insensitive")]
    Insensitive,
}

impl FromStr for CaseHandling {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "insensitive" => Ok(CaseHandling::Insensitive),
            "sensitive" => Ok(CaseHandling::Sensitive),
            _ => Err(format!("invalid case mode: {}", s)),
        }
    }
}

/// Typed `p4 info` result, deserialized from the tagged-protocol record.
///
/// Field names use the server's tagged keys (`userName`, `serverVersion`, ...)
/// -- the stable machine interface -- rather than the human-formatted labels
/// of untagged output ("User name: ..."), which are presentation text.
/// Fields that a server may omit (e.g. no client spec, unlicensed server) are
/// Options.
#[derive(Serialize, Deserialize, Debug)]
pub struct Info {
    #[serde(rename = "caseHandling")]
    pub case_handling: CaseHandling,

    #[serde(rename = "clientAddress")]
    pub client_address: String,

    #[serde(rename = "clientHost")]
    pub client_host: String,

    #[serde(rename = "clientName")]
    pub client_name: String,
    #[serde(rename = "clientRoot", default, deserialize_with = "optional_string")]
    pub client_root: Option<String>,
    #[serde(rename = "clientCwd")]
    pub current_dir: PathBuf,

    #[serde(rename = "serverAddress")]
    pub server_address: String,
    #[serde(rename = "serverRoot")]
    pub server_root: String,
    #[serde(rename = "serverDate")]
    pub server_date: String,
    #[serde(rename = "serverVersion")]
    pub server_version: String,
    #[serde(rename = "serverUptime")]
    pub server_uptime: String,

    #[serde(
        rename = "serverLicense",
        default,
        deserialize_with = "optional_string"
    )]
    pub server_license: Option<String>,

    #[serde(rename = "userName")]
    pub user_name: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::de::value::MapDeserializer;
    use std::collections::HashMap;

    #[test]
    fn info_deserializes_from_tagged_record() {
        let m: HashMap<String, String> = [
            ("caseHandling", "insensitive"),
            ("clientAddress", "127.0.0.1:54321"),
            ("clientHost", "myhost"),
            ("clientName", "myhost-ws"),
            ("clientCwd", "/work"),
            ("serverAddress", "localhost:1666"),
            ("serverRoot", "/srv/p4"),
            ("serverDate", "2026/07/18 12:00:00 +0000 UTC"),
            ("serverVersion", "P4D/NTX64/2025.2/2907753 (2026/03/10)"),
            ("serverUptime", "00:00:03"),
            ("serverLicense", "none"),
            ("userName", "alice"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();

        let info = Info::deserialize(MapDeserializer::<_, serde::de::value::Error>::new(
            m.into_iter(),
        ))
        .expect("deserialize tagged info record");
        assert_eq!(info.user_name, "alice");
        assert_eq!(info.server_address, "localhost:1666");
        // Optional field absent from the record deserializes as None...
        assert!(info.client_root.is_none());
        // ...and present as a plain string becomes Some (a live server reports
        // serverLicense: "none" when unlicensed -- this used to fail with
        // "invalid type: string, expected option").
        assert_eq!(info.server_license.as_deref(), Some("none"));
    }

    #[test]
    fn shortened_option_maps_to_dash_s() {
        assert_eq!(Options::new().get_args(), Vec::<String>::new());
        assert_eq!(
            Options::new().shortened().get_args(),
            vec!["-s".to_string()]
        );
    }
}
