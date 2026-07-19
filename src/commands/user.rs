//! Typed `p4 user` -- read (`-o`) and write (`-i`) a user spec.

use crate::client::{Client, UserInterface};
use crate::commands::helpers::{opt_from_str, optional_string};
use crate::commands::users::UserType;
use crate::errors::Error;
use serde::Deserialize;
use std::collections::HashMap;
use std::fmt::Display;

impl Client {
    /// Read a user spec (`p4 user -o [name]`), typed. With `None`, the
    /// connection's own user is used. For a user that doesn't exist yet the
    /// server returns a defaulted template -- the create flow is: read the
    /// template, modify it, save it.
    pub fn user_spec(&mut self, name: Option<&str>) -> Result<UserSpec, Error> {
        let mut ui = UserInterface::new();
        let mut args = vec!["-o".to_string()];
        if let Some(name) = name {
            args.push(name.to_string());
        }
        let mut records = self.run_records(&mut ui, "user", args)?;
        let record = if records.is_empty() {
            HashMap::new()
        } else {
            records.swap_remove(0)
        };
        UserSpec::from_record(record)
    }

    /// Create or update a user spec (`p4 user -i`). Server-managed fields
    /// (`Update`/`Access`) are not sent.
    pub fn save_user_spec(&mut self, spec: &UserSpec) -> Result<(), Error> {
        let mut ui = UserInterface::new();
        ui.set_input(&spec.to_spec_text());
        self.run_records(&mut ui, "user", vec!["-i".to_string()])?;
        Ok(())
    }
}

/// A user spec, as read by `user -o` / written by `user -i`.
///
/// Server-managed fields (`Update`/`Access`) are kept as the raw formatted-date
/// strings the server reports and are never sent back. `AuthMethod` is passed
/// through untyped (it grows values -- `perforce`, `ldap`, `perforce+2fa`, ...);
/// `Type` reuses the open [`UserType`] enum so an unknown type is preserved
/// rather than fatal.
#[derive(Debug, Clone, Deserialize)]
pub struct UserSpec {
    #[serde(rename = "User")]
    pub user: String,

    /// Required by the user form; the `user -o` template always supplies it.
    #[serde(rename = "Email")]
    pub email: String,

    /// Required by the user form; the `user -o` template always supplies it.
    #[serde(rename = "FullName")]
    pub full_name: String,

    /// e.g. "standard"; open enum, round-tripped.
    #[serde(rename = "Type", default, deserialize_with = "opt_from_str")]
    pub user_type: Option<UserType>,

    /// e.g. "perforce"; round-tripped untyped.
    #[serde(rename = "AuthMethod", default, deserialize_with = "optional_string")]
    pub auth_method: Option<String>,

    /// Optional review-daemon path filter.
    #[serde(rename = "JobView", default, deserialize_with = "optional_string")]
    pub job_view: Option<String>,

    /// Write-only on save; the server never echoes a cleartext password on
    /// `-o`, so this is normally `None` when read.
    #[serde(rename = "Password", default, deserialize_with = "optional_string")]
    pub password: Option<String>,

    /// Server-managed formatted date (e.g. "2026/07/19 07:41:36"); never sent
    /// back on save. (In the `users` list command these are epoch seconds, but
    /// the spec form reports formatted dates.)
    #[serde(rename = "Update", default, deserialize_with = "optional_string")]
    pub update: Option<String>,

    /// Server-managed formatted date; never sent back on save.
    #[serde(rename = "Access", default, deserialize_with = "optional_string")]
    pub access: Option<String>,

    /// The review paths, from the `Reviews0..ReviewsN` record keys.
    #[serde(skip)]
    pub reviews: Vec<String>,
}

impl UserSpec {
    /// Build a typed spec from a tagged `user -o` record.
    pub fn from_record(mut record: HashMap<String, String>) -> Result<UserSpec, Error> {
        // Spec-output bookkeeping, not a field.
        record.remove("specFormatted");

        let mut reviews = Vec::new();
        let mut i = 0;
        while let Some(line) = record.remove(&format!("Reviews{i}")) {
            reviews.push(line);
            i += 1;
        }

        let mut spec = UserSpec::deserialize(serde::de::value::MapDeserializer::new(
            record.clone().into_iter(),
        ))
        .map_err(|e| Error::SerializationError(e, record))?;
        spec.reviews = reviews;
        Ok(spec)
    }

    /// Render the spec as the text form `user -i` reads. Server-managed fields
    /// (Update/Access) are omitted.
    pub fn to_spec_text(&self) -> String {
        let mut out = String::new();

        let single = |out: &mut String, name: &str, value: &dyn Display| {
            out.push_str(&format!("{name}:\t{value}\n\n"));
        };
        let multi = |out: &mut String, name: &str, lines: &mut dyn Iterator<Item = String>| {
            out.push_str(name);
            out.push_str(":\n");
            for line in lines {
                out.push('\t');
                out.push_str(&line);
                out.push('\n');
            }
            out.push('\n');
        };

        single(&mut out, "User", &self.user);
        single(&mut out, "Email", &self.email);
        single(&mut out, "FullName", &self.full_name);
        if let Some(user_type) = &self.user_type {
            single(&mut out, "Type", user_type);
        }
        if let Some(auth_method) = &self.auth_method {
            single(&mut out, "AuthMethod", auth_method);
        }
        if let Some(job_view) = &self.job_view {
            single(&mut out, "JobView", job_view);
        }
        if let Some(password) = &self.password {
            single(&mut out, "Password", password);
        }
        if !self.reviews.is_empty() {
            multi(&mut out, "Reviews", &mut self.reviews.iter().cloned());
        }

        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn captured_record() -> HashMap<String, String> {
        // Verbatim shape from a real tagged `user -o` (p4d 2022.2): the spec
        // form reports Update/Access as formatted date strings (not epochs),
        // and even a not-yet-saved template carries them.
        [
            ("User", "danrs"),
            ("Type", "standard"),
            ("Email", "danrs@Dan-Desktop"),
            ("Update", "2026/07/19 08:48:53"),
            ("Access", "2026/07/19 08:48:53"),
            ("FullName", "danrs"),
            ("AuthMethod", "perforce"),
            ("specFormatted", ""),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
    }

    #[test]
    fn user_spec_from_captured_record() {
        let spec = UserSpec::from_record(captured_record()).expect("parse captured record");
        assert_eq!(spec.user, "danrs");
        assert_eq!(spec.email, "danrs@Dan-Desktop");
        assert_eq!(spec.full_name, "danrs");
        assert_eq!(spec.user_type, Some(UserType::Standard));
        assert_eq!(spec.auth_method.as_deref(), Some("perforce"));
        // Spec-form dates are the raw formatted strings, kept verbatim.
        assert_eq!(spec.update.as_deref(), Some("2026/07/19 08:48:53"));
        assert!(spec.job_view.is_none());
        assert!(spec.password.is_none());
        assert!(spec.reviews.is_empty());
    }

    #[test]
    fn reviews_parsed_from_indexed_keys() {
        let mut record = captured_record();
        record.insert("Reviews0".to_string(), "//depot/main/...".to_string());
        record.insert("Reviews1".to_string(), "//depot/dev/...".to_string());

        let spec = UserSpec::from_record(record).expect("parse record with reviews");
        assert_eq!(spec.reviews, vec!["//depot/main/...", "//depot/dev/..."]);
    }

    #[test]
    fn spec_text_round_trips_the_essentials() {
        let mut spec = UserSpec::from_record(captured_record()).unwrap();
        spec.email = "dan@new.example".to_string();
        spec.reviews.push("//depot/main/...".to_string());

        let text = spec.to_spec_text();
        assert!(text.contains("User:\tdanrs\n"));
        assert!(text.contains("Email:\tdan@new.example\n"));
        assert!(text.contains("FullName:\tdanrs\n"));
        assert!(text.contains("Type:\tstandard\n"));
        assert!(text.contains("AuthMethod:\tperforce\n"));
        assert!(text.contains("Reviews:\n\t//depot/main/...\n"));
        // Server-managed fields are never written.
        assert!(!text.contains("Update:"));
        assert!(!text.contains("Access:"));
    }

    #[test]
    fn unknown_user_type_is_preserved() {
        let mut record = captured_record();
        record.insert("Type".to_string(), "background-bot".to_string());
        let spec = UserSpec::from_record(record).expect("parse record");
        assert_eq!(
            spec.user_type,
            Some(UserType::Other("background-bot".to_string()))
        );
        // ...and it survives the render.
        assert!(spec.to_spec_text().contains("Type:\tbackground-bot\n"));
    }
}
