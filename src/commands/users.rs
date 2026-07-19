//! Typed `p4 users` -- list the users known to the server.

use crate::client::{Client, UserInterface};
use crate::commands::helpers::{from_str_value, opt_from_str};
use crate::errors::Error;
use serde::Deserialize;
use std::fmt::{self, Display, Formatter};
use std::str::FromStr;

impl Client {
    /// List the server's users (`p4 users`), typed.
    pub fn users(&mut self, options: &Options) -> Result<Vec<User>, Error> {
        let mut ui = UserInterface::new();
        let records = self.run_records(&mut ui, "users", options.get_args())?;
        records
            .into_iter()
            .map(|m| {
                User::deserialize(serde::de::value::MapDeserializer::new(
                    m.clone().into_iter(),
                ))
                .map_err(|e| Error::SerializationError(e, m))
            })
            .collect()
    }
}

#[derive(Debug, Default)]
pub struct Options {
    max: Option<u32>,
    include_service: bool,
}

impl Options {
    pub fn new() -> Options {
        Options::default()
    }

    /// `-m max`: list only the first `max` users.
    pub fn max(mut self, max: u32) -> Self {
        self.max = Some(max);
        self
    }

    /// `-a`: include service and operator users in the listing.
    pub fn include_service(mut self) -> Self {
        self.include_service = true;
        self
    }

    pub fn get_args(&self) -> Vec<String> {
        let mut args = Vec::new();
        if self.include_service {
            args.push("-a".to_string());
        }
        if let Some(max) = self.max {
            args.push("-m".to_string());
            args.push(max.to_string());
        }
        args
    }
}

/// The kind of a user account. Open-ended (`Other`) because servers have grown
/// new types over the years and listing users should not fail on one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UserType {
    Standard,
    Operator,
    Service,
    Other(String),
}

impl FromStr for UserType {
    type Err = std::convert::Infallible;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "standard" => UserType::Standard,
            "operator" => UserType::Operator,
            "service" => UserType::Service,
            other => UserType::Other(other.to_string()),
        })
    }
}

impl Display for UserType {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            UserType::Standard => f.write_str("standard"),
            UserType::Operator => f.write_str("operator"),
            UserType::Service => f.write_str("service"),
            UserType::Other(s) => f.write_str(s),
        }
    }
}

/// One user from `p4 users`, deserialized from the tagged record. In tagged
/// output `Update`/`Access` are epoch seconds (unlike the formatted dates the
/// spec form shows).
#[derive(Debug, Clone, Deserialize)]
pub struct User {
    #[serde(rename = "User")]
    pub user: String,

    #[serde(rename = "Email")]
    pub email: String,

    #[serde(rename = "FullName")]
    pub full_name: String,

    /// Absent on very old servers, hence optional.
    #[serde(rename = "Type", default, deserialize_with = "opt_from_str")]
    pub user_type: Option<UserType>,

    /// Last spec update, seconds since the Unix epoch.
    #[serde(rename = "Update", deserialize_with = "from_str_value")]
    pub update: u64,

    /// Last access, seconds since the Unix epoch.
    #[serde(rename = "Access", deserialize_with = "from_str_value")]
    pub access: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::de::value::MapDeserializer;
    use std::collections::HashMap;

    /// Field shapes captured from a real `p4 users` tagged record (p4d 2022.2).
    #[test]
    fn user_deserializes_from_captured_record() {
        let m: HashMap<String, String> = [
            ("User", "danrs"),
            ("Email", "danrs@example.com"),
            ("FullName", "Dan Test"),
            ("Type", "standard"),
            ("Update", "1784461296"),
            ("Access", "1784461296"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();

        let u = User::deserialize(MapDeserializer::<_, serde::de::value::Error>::new(
            m.into_iter(),
        ))
        .expect("deserialize captured users record");
        assert_eq!(u.user, "danrs");
        assert_eq!(u.email, "danrs@example.com");
        assert_eq!(u.user_type, Some(UserType::Standard));
        assert_eq!(u.update, 1_784_461_296);
        assert_eq!(u.access, 1_784_461_296);
    }

    #[test]
    fn unknown_user_type_is_preserved_not_fatal() {
        assert_eq!(
            "background-bot".parse::<UserType>().unwrap(),
            UserType::Other("background-bot".to_string())
        );
        assert_eq!(UserType::Service.to_string(), "service");
    }

    #[test]
    fn options_build_expected_args() {
        assert!(Options::new().get_args().is_empty());
        assert_eq!(
            Options::new().include_service().max(5).get_args(),
            vec!["-a", "-m", "5"]
        );
    }
}
