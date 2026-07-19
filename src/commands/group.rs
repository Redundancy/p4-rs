//! Typed `p4 group` / `p4 groups` -- list groups and read/write group specs.

use crate::client::{Client, UserInterface};
use crate::commands::helpers::{from_str_value, opt_from_str, optional_string};
use crate::errors::Error;
use serde::{Deserialize, Deserializer};
use std::collections::HashMap;
use std::fmt::{self, Display, Formatter};
use std::str::FromStr;

/// Deserialize a tagged `"0"`/`"1"` flag into a bool. The `groups` listing
/// reports its membership flags this way; absent (older servers) defaults to
/// false via `#[serde(default)]`.
fn bool_from_01<'de, D>(d: D) -> Result<bool, D::Error>
where
    D: Deserializer<'de>,
{
    Ok(String::deserialize(d)? == "1")
}

impl Client {
    /// List the server's groups (`p4 groups`), typed. A fresh server has no
    /// groups, so this returns an empty vector until one is created.
    pub fn groups(&mut self) -> Result<Vec<GroupSummary>, Error> {
        let mut ui = UserInterface::new();
        let records = self.run_records(&mut ui, "groups", Vec::new())?;
        records
            .into_iter()
            .map(|m| {
                GroupSummary::deserialize(serde::de::value::MapDeserializer::new(
                    m.clone().into_iter(),
                ))
                .map_err(|e| Error::SerializationError(e, m))
            })
            .collect()
    }

    /// Read a group spec (`p4 group -o name`), typed. For a group that doesn't
    /// exist yet the server returns a defaulted template -- the create flow is:
    /// read the template, add users/owners/subgroups, save it.
    pub fn group_spec(&mut self, name: &str) -> Result<GroupSpec, Error> {
        let mut ui = UserInterface::new();
        let mut records =
            self.run_records(&mut ui, "group", vec!["-o".to_string(), name.to_string()])?;
        let record = if records.is_empty() {
            HashMap::new()
        } else {
            records.swap_remove(0)
        };
        GroupSpec::from_record(record)
    }

    /// Create or update a group spec (`p4 group -i`). A group must have at least
    /// one user, owner, or subgroup for the server to create it.
    pub fn save_group_spec(&mut self, spec: &GroupSpec) -> Result<(), Error> {
        let mut ui = UserInterface::new();
        ui.set_input(&spec.to_spec_text());
        self.run_records(&mut ui, "group", vec!["-i".to_string()])?;
        Ok(())
    }
}

/// A group's per-command resource limit or timeout (`MaxResults`,
/// `MaxScanRows`, `Timeout`, ...). These fields carry either a numeric limit or
/// one of the sentinels `unset` (no group-specific limit) / `unlimited`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Limit {
    /// No group-specific value (`unset`).
    #[default]
    Unset,
    /// Explicitly no limit (`unlimited`).
    Unlimited,
    /// A concrete numeric limit.
    Value(u64),
}

impl FromStr for Limit {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "unset" => Ok(Limit::Unset),
            "unlimited" => Ok(Limit::Unlimited),
            other => other
                .parse::<u64>()
                .map(Limit::Value)
                .map_err(|_| format!("unknown Limit value: {other:?}")),
        }
    }
}

impl Display for Limit {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Limit::Unset => f.write_str("unset"),
            Limit::Unlimited => f.write_str("unlimited"),
            Limit::Value(n) => write!(f, "{n}"),
        }
    }
}

/// One row of `p4 groups`. The listing is membership-oriented: it reports, per
/// (group, user) pair, the group name, the user, and whether that user is a
/// direct member, an owner, and/or reached via a subgroup. The resource-limit
/// columns the server also emits are left untyped here (they use `0` for
/// unset, unlike the spec form's `unset` sentinel) -- read them via
/// [`Client::group_spec`] when the typed [`Limit`] distinction matters.
#[derive(Debug, Clone, Deserialize)]
pub struct GroupSummary {
    #[serde(rename = "group")]
    pub group: String,

    /// The user this membership row is about. Absent on servers whose `groups`
    /// output predates per-user rows.
    #[serde(rename = "user", default, deserialize_with = "optional_string")]
    pub user: Option<String>,

    /// This user is a direct member of the group.
    #[serde(rename = "isUser", default, deserialize_with = "bool_from_01")]
    pub is_user: bool,

    /// This user is an owner of the group.
    #[serde(rename = "isOwner", default, deserialize_with = "bool_from_01")]
    pub is_owner: bool,

    /// This user's membership is via a nested subgroup.
    #[serde(rename = "isSubGroup", default, deserialize_with = "bool_from_01")]
    pub is_sub_group: bool,
}

/// A group spec, as read by `group -o` / written by `group -i`.
///
/// The `Max*`/`Timeout`/`PasswordTimeout` fields use [`Limit`]. `MaxMemory` and
/// `IdleTimeout` exist only on newer servers and are optional. LDAP-sync fields
/// are round-tripped untyped so a read-modify-write cycle never drops them.
/// Group specs have no server-managed `Update`/`Access` stamps.
#[derive(Debug, Clone, Deserialize)]
pub struct GroupSpec {
    #[serde(rename = "Group")]
    pub group: String,

    /// Free-form description. Newer servers only; may span multiple lines.
    #[serde(rename = "Description", default, deserialize_with = "optional_string")]
    pub description: Option<String>,

    #[serde(rename = "MaxResults", deserialize_with = "from_str_value")]
    pub max_results: Limit,

    #[serde(rename = "MaxScanRows", deserialize_with = "from_str_value")]
    pub max_scan_rows: Limit,

    #[serde(rename = "MaxLockTime", deserialize_with = "from_str_value")]
    pub max_lock_time: Limit,

    #[serde(rename = "MaxOpenFiles", deserialize_with = "from_str_value")]
    pub max_open_files: Limit,

    /// Newer servers only; round-tripped when present.
    #[serde(rename = "MaxMemory", default, deserialize_with = "opt_from_str")]
    pub max_memory: Option<Limit>,

    #[serde(rename = "Timeout", deserialize_with = "from_str_value")]
    pub timeout: Limit,

    #[serde(rename = "PasswordTimeout", deserialize_with = "from_str_value")]
    pub password_timeout: Limit,

    /// Conditional; present only on servers/configs that report it.
    #[serde(rename = "IdleTimeout", default, deserialize_with = "opt_from_str")]
    pub idle_timeout: Option<Limit>,

    /// LDAP-sync configuration name; present only for LDAP-synced groups.
    #[serde(rename = "LdapConfig", default, deserialize_with = "optional_string")]
    pub ldap_config: Option<String>,

    /// LDAP search query; present only for LDAP-synced groups.
    #[serde(
        rename = "LdapSearchQuery",
        default,
        deserialize_with = "optional_string"
    )]
    pub ldap_search_query: Option<String>,

    /// LDAP user attribute; present only for LDAP-synced groups.
    #[serde(
        rename = "LdapUserAttribute",
        default,
        deserialize_with = "optional_string"
    )]
    pub ldap_user_attribute: Option<String>,

    /// Users directly in the group, from the `Users0..UsersN` record keys.
    #[serde(skip)]
    pub users: Vec<String>,

    /// Group owners, from the `Owners0..OwnersN` record keys.
    #[serde(skip)]
    pub owners: Vec<String>,

    /// Nested subgroups, from the `Subgroups0..SubgroupsN` record keys.
    #[serde(skip)]
    pub subgroups: Vec<String>,
}

/// Pull an indexed list (`Field0`, `Field1`, ...) out of a tagged record,
/// removing the keys as it goes -- the group analogue of the client `View`
/// loop.
fn take_indexed(record: &mut HashMap<String, String>, field: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(v) = record.remove(&format!("{field}{i}")) {
        out.push(v);
        i += 1;
    }
    out
}

impl GroupSpec {
    /// Build a typed spec from a tagged `group -o` record.
    pub fn from_record(mut record: HashMap<String, String>) -> Result<GroupSpec, Error> {
        // Spec-output bookkeeping, not fields.
        record.remove("specFormatted");
        record.remove("func");
        record.remove("specdef");

        let users = take_indexed(&mut record, "Users");
        let owners = take_indexed(&mut record, "Owners");
        let subgroups = take_indexed(&mut record, "Subgroups");

        let mut spec = GroupSpec::deserialize(serde::de::value::MapDeserializer::new(
            record.clone().into_iter(),
        ))
        .map_err(|e| Error::SerializationError(e, record))?;
        spec.users = users;
        spec.owners = owners;
        spec.subgroups = subgroups;
        Ok(spec)
    }

    /// Render the spec as the text form `group -i` reads.
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

        single(&mut out, "Group", &self.group);
        if let Some(description) = self.description.as_deref().filter(|d| !d.is_empty()) {
            multi(
                &mut out,
                "Description",
                &mut description.lines().map(str::to_string),
            );
        }
        single(&mut out, "MaxResults", &self.max_results);
        single(&mut out, "MaxScanRows", &self.max_scan_rows);
        single(&mut out, "MaxLockTime", &self.max_lock_time);
        single(&mut out, "MaxOpenFiles", &self.max_open_files);
        if let Some(max_memory) = &self.max_memory {
            single(&mut out, "MaxMemory", max_memory);
        }
        single(&mut out, "Timeout", &self.timeout);
        single(&mut out, "PasswordTimeout", &self.password_timeout);
        if let Some(idle_timeout) = &self.idle_timeout {
            single(&mut out, "IdleTimeout", idle_timeout);
        }
        if let Some(ldap_config) = &self.ldap_config {
            single(&mut out, "LdapConfig", ldap_config);
        }
        if let Some(ldap_search_query) = &self.ldap_search_query {
            single(&mut out, "LdapSearchQuery", ldap_search_query);
        }
        if let Some(ldap_user_attribute) = &self.ldap_user_attribute {
            single(&mut out, "LdapUserAttribute", ldap_user_attribute);
        }

        multi(&mut out, "Subgroups", &mut self.subgroups.iter().cloned());
        multi(&mut out, "Owners", &mut self.owners.iter().cloned());
        multi(&mut out, "Users", &mut self.users.iter().cloned());

        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Verbatim shape from a real tagged `group -o newgrp` template (p4d
    /// 2025.2): a fresh group has no Users/Owners/Subgroups keys, the Max*
    /// fields are `unset`, and Timeout defaults to 43200.
    fn captured_template() -> HashMap<String, String> {
        [
            ("Group", "newgrp"),
            ("Description", ""),
            ("MaxResults", "unset"),
            ("MaxScanRows", "unset"),
            ("MaxLockTime", "unset"),
            ("MaxOpenFiles", "unset"),
            ("MaxMemory", "unset"),
            ("Timeout", "43200"),
            ("PasswordTimeout", "unset"),
            ("specFormatted", ""),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
    }

    /// A saved group with members, owners and a numeric MaxResults.
    fn captured_saved() -> HashMap<String, String> {
        [
            ("Group", "devs"),
            ("MaxResults", "1000"),
            ("MaxScanRows", "unset"),
            ("MaxLockTime", "unlimited"),
            ("MaxOpenFiles", "unset"),
            ("MaxMemory", "unset"),
            ("Timeout", "43200"),
            ("PasswordTimeout", "unset"),
            ("Owners0", "alice"),
            ("Users0", "alice"),
            ("Users1", "bob"),
            ("Subgroups0", "leads"),
            ("specFormatted", ""),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
    }

    /// Verbatim shape from a real tagged `groups` record (p4d 2025.2): one
    /// membership row, lowercase keys, `0`/`1` flags.
    fn captured_list_row() -> HashMap<String, String> {
        [
            ("group", "newgrp"),
            ("user", "danrs"),
            ("isOwner", "0"),
            ("isSubGroup", "0"),
            ("isUser", "1"),
            ("maxLockTime", "0"),
            ("maxMemory", "0"),
            ("maxOpenFiles", "0"),
            ("maxResults", "0"),
            ("maxScanRows", "0"),
            ("passTimeout", "0"),
            ("timeout", "43200"),
            ("description", ""),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
    }

    #[test]
    fn group_summary_from_list_row() {
        let s = GroupSummary::deserialize(serde::de::value::MapDeserializer::<
            _,
            serde::de::value::Error,
        >::new(captured_list_row().into_iter()))
        .expect("deserialize captured list row");
        assert_eq!(s.group, "newgrp");
        assert_eq!(s.user.as_deref(), Some("danrs"));
        assert!(s.is_user);
        assert!(!s.is_owner);
        assert!(!s.is_sub_group);
    }

    #[test]
    fn limit_parses_and_round_trips() {
        assert_eq!("unset".parse::<Limit>().unwrap(), Limit::Unset);
        assert_eq!("unlimited".parse::<Limit>().unwrap(), Limit::Unlimited);
        assert_eq!("1000".parse::<Limit>().unwrap(), Limit::Value(1000));
        assert!("bogus".parse::<Limit>().is_err());

        for s in ["unset", "unlimited", "0", "1000"] {
            assert_eq!(s.parse::<Limit>().unwrap().to_string(), s);
        }
    }

    #[test]
    fn group_spec_from_template_record() {
        let spec = GroupSpec::from_record(captured_template()).expect("parse template");
        assert_eq!(spec.group, "newgrp");
        assert_eq!(spec.max_results, Limit::Unset);
        assert_eq!(spec.max_memory, Some(Limit::Unset));
        assert_eq!(spec.timeout, Limit::Value(43200));
        assert_eq!(spec.password_timeout, Limit::Unset);
        assert!(spec.idle_timeout.is_none());
        assert!(spec.users.is_empty());
        assert!(spec.owners.is_empty());
        assert!(spec.subgroups.is_empty());
    }

    #[test]
    fn group_spec_from_saved_record_collects_indexed_lists() {
        let spec = GroupSpec::from_record(captured_saved()).expect("parse saved");
        assert_eq!(spec.group, "devs");
        assert_eq!(spec.max_results, Limit::Value(1000));
        assert_eq!(spec.max_lock_time, Limit::Unlimited);
        assert_eq!(spec.owners, vec!["alice"]);
        assert_eq!(spec.users, vec!["alice", "bob"]);
        assert_eq!(spec.subgroups, vec!["leads"]);
    }

    #[test]
    fn spec_text_renders_fields_and_lists() {
        let mut spec = GroupSpec::from_record(captured_saved()).unwrap();
        spec.description = Some("Dev team.\nSecond line.".to_string());
        let text = spec.to_spec_text();

        assert!(text.contains("Group:\tdevs\n"));
        assert!(text.contains("Description:\n\tDev team.\n\tSecond line.\n"));
        assert!(text.contains("MaxResults:\t1000\n"));
        assert!(text.contains("MaxLockTime:\tunlimited\n"));
        assert!(text.contains("MaxMemory:\tunset\n"));
        assert!(text.contains("Timeout:\t43200\n"));
        assert!(text.contains("Owners:\n\talice\n"));
        assert!(text.contains("Users:\n\talice\n\tbob\n"));
        assert!(text.contains("Subgroups:\n\tleads\n"));
    }

    #[test]
    fn spec_text_omits_absent_optional_fields() {
        // A template without MaxMemory (older server) omits it entirely.
        let mut rec = captured_template();
        rec.remove("MaxMemory");
        let spec = GroupSpec::from_record(rec).unwrap();
        assert!(spec.max_memory.is_none());

        let text = spec.to_spec_text();
        assert!(!text.contains("MaxMemory:"));
        assert!(!text.contains("IdleTimeout:"));
        assert!(!text.contains("LdapConfig:"));
    }
}
