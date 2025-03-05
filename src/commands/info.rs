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

#[derive(Serialize, Deserialize, Debug)]
pub struct Info {
    #[serde(rename = "Case Handling")]
    pub case_handling: CaseHandling,
    
    #[serde(rename = "Client address")]
    pub client_address: String,
    
    #[serde(rename = "Client host")]
    pub client_host: String,
    
    #[serde(rename = "Client name")]
    pub client_name: String,
    #[serde(rename = "Client root")]
    pub client_root: Option<String>,
    #[serde(rename = "Current directory")]
    pub current_dir: PathBuf,

    #[serde(rename = "Server address")]
    pub server_address: String,
    #[serde(rename = "Server root")]
    pub server_root: String,
    #[serde(rename = "Server date")]
    pub server_date: String,
    #[serde(rename = "Server version")]
    pub server_version: String,
    #[serde(rename = "Server uptime")]
    pub server_uptime: String,

    #[serde(rename = "User name")]
    pub user_name: String,

}