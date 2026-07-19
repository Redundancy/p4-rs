use std::path::PathBuf;
use std::str::FromStr;
use serde::{Serialize, Deserialize};


pub struct Options {
    short: bool,
}

impl Options {
    pub fn new() -> Options {
        Options {
            short: false,
        }
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
    #[serde(rename = "clientRoot")]
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

    #[serde(rename = "serverLicense")]
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
            ("userName", "alice"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();

        let info =
            Info::deserialize(MapDeserializer::<_, serde::de::value::Error>::new(m.into_iter()))
                .expect("deserialize tagged info record");
        assert_eq!(info.user_name, "alice");
        assert_eq!(info.server_address, "localhost:1666");
        // Optional fields absent from the record deserialize as None.
        assert!(info.client_root.is_none());
        assert!(info.server_license.is_none());
    }

    #[test]
    fn shortened_option_maps_to_dash_s() {
        assert_eq!(Options::new().get_args(), Vec::<String>::new());
        assert_eq!(Options::new().shortened().get_args(), vec!["-s".to_string()]);
    }
}