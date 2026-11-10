//! A private cache of the embedded Tailscale node's peer roster.
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::PathBuf};

#[derive(Clone, Serialize, Deserialize)]
pub struct Machine {
    pub name: String,
    pub hostname: String,
    pub address: String,
    pub os: String,
    pub online: bool,
}

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct Roster {
    pub machines: Vec<Machine>,
    #[serde(default)]
    pub updated_at: u64,
}

#[derive(Deserialize)]
struct Status {
    #[serde(rename = "Self")]
    own: Option<Node>,
    #[serde(rename = "Peer", default)]
    peers: Option<BTreeMap<String, Node>>,
}
#[derive(Deserialize)]
struct Node {
    #[serde(rename = "HostName", default)]
    name: String,
    #[serde(rename = "DNSName", default)]
    dns: String,
    #[serde(rename = "OS", default)]
    os: String,
    #[serde(rename = "Online", default)]
    online: bool,
    #[serde(rename = "TailscaleIPs", default)]
    ips: Vec<String>,
}

pub(crate) fn data_dir() -> PathBuf {
    #[cfg(target_os = "android")]
    { crate::android_terminal::data_dir() }
    #[cfg(not(target_os = "android"))]
    { PathBuf::from(std::env::var_os("HOME").unwrap()).join("Documents") }
}

impl Roster {
    pub fn path() -> PathBuf {
        data_dir().join("tailnet-machines.json")
    }
    pub fn load() -> Self {
        std::fs::read(Self::path())
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }
    pub fn from_status(json: &str) -> anyhow::Result<Self> {
        let status: Status = serde_json::from_str(json)?;
        let mut machines = Vec::new();
        for node in status
            .own
            .into_iter()
            .chain(status.peers.unwrap_or_default().into_values())
        {
            if ["ios", "android", "tvos"].contains(&node.os.to_lowercase().as_str()) {
                continue;
            }
            let address = node
                .ips
                .iter()
                .find(|ip| !ip.contains(':'))
                .or(node.ips.first())
                .cloned()
                .unwrap_or_default();
            if address.is_empty() {
                continue;
            }
            let hostname = node.dns.trim_end_matches('.').to_owned();
            let name = if node.name == "localhost" || node.name.is_empty() {
                hostname.split('.').next().unwrap_or(&hostname).to_owned()
            } else {
                node.name
            };
            machines.push(Machine {
                name,
                hostname,
                address,
                os: node.os,
                online: node.online,
            });
        }
        machines.sort_by(|a, b| {
            b.online
                .cmp(&a.online)
                .then(a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        machines.dedup_by(|a, b| a.address == b.address);
        Ok(Self {
            machines,
            updated_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_secs(),
        })
    }
    pub fn save(&self) -> anyhow::Result<()> {
        let path = Self::path();
        std::fs::create_dir_all(path.parent().unwrap())?;
        let temp = path.with_extension("tmp");
        std::fs::write(&temp, serde_json::to_vec_pretty(self)?)?;
        std::fs::rename(temp, path)?;
        Ok(())
    }
}
