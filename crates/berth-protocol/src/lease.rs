use serde::{Deserialize, Serialize};

use crate::quote::Quote;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Os {
    Linux,
    Windows,
    Macos,
}

impl Os {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Linux => "linux",
            Self::Windows => "windows",
            Self::Macos => "macos",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Class {
    #[serde(rename = "private")]
    Private,
    #[serde(rename = "licensed-cloud")]
    LicensedCloud,
    #[serde(rename = "mesh")]
    Mesh,
}

impl Class {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Private => "private",
            Self::LicensedCloud => "licensed-cloud",
            Self::Mesh => "mesh",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum License {
    #[serde(rename = "linux")]
    Linux,
    #[serde(rename = "w365-agents")]
    W365Agents,
    #[serde(rename = "avd-external")]
    AvdExternal,
    #[serde(rename = "avd-multisession")]
    AvdMultisession,
    #[serde(rename = "eval")]
    Eval,
    #[serde(rename = "apple-private")]
    ApplePrivate,
    #[serde(rename = "apple-section-3")]
    AppleSection3,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Density {
    Shared,
    #[default]
    Isolated,
    Exclusive,
}

impl Density {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Shared => "shared",
            Self::Isolated => "isolated",
            Self::Exclusive => "exclusive",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Term {
    #[default]
    OnDemand,
    Monthly,
    Annual,
}

impl Term {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::OnDemand => "on_demand",
            Self::Monthly => "monthly",
            Self::Annual => "annual",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Isolation {
    #[default]
    Vm,
    Hypervisor,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Egress {
    #[default]
    Allowlist,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Resources {
    pub vcpu: u32,
    pub mem_gib: u32,
    pub disk_gib: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Workspace {
    pub id: String,
    pub disk_gib: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObjectStore {
    pub endpoint: String,
    pub bucket: String,
    pub prefix: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Network {
    pub egress: Egress,
    #[serde(default)]
    pub domains: Vec<String>,
}

/// Default guest egress hosts. Unset `BERTH_ALLOWLIST` / omitted config key uses this.
pub const DEFAULT_ALLOWLIST: &str = "github.com,pypi.org,registry.npmjs.org";

/// Parse a comma-separated allowlist.
///
/// * `None` → default hosts
/// * `Some("")` / whitespace → deny-all (no hosts)
/// * anything else → sanitized hosts (invalid tokens dropped)
pub fn parse_allowlist(raw: Option<&str>) -> Vec<String> {
    match raw {
        None => parse_allowlist_csv(DEFAULT_ALLOWLIST),
        Some(s) => parse_allowlist_csv(s),
    }
}

/// Config/CLI key: missing (`None`) means the node applies env/default.
/// Present, including `""`, is sent as lease `network.domains`.
pub fn network_from_allowlist_key(raw: Option<&str>) -> Option<Network> {
    raw.map(|s| Network {
        egress: Egress::Allowlist,
        domains: parse_allowlist(Some(s)),
    })
}

fn parse_allowlist_csv(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(|s| s.trim().to_ascii_lowercase())
        .filter(|s| valid_host(s))
        .collect()
}

fn valid_host(s: &str) -> bool {
    if s.is_empty() || s.len() > 253 {
        return false;
    }
    if s.chars().all(|c| c.is_ascii_digit() || c == '.') {
        let mut n = 0;
        for octet in s.split('.') {
            n += 1;
            if n > 4 || octet.parse::<u8>().is_err() {
                return false;
            }
        }
        return n == 4;
    }
    s.split('.').all(valid_dns_label)
}

fn valid_dns_label(label: &str) -> bool {
    !label.is_empty()
        && label.len() <= 63
        && !label.starts_with('-')
        && !label.ends_with('-')
        && label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

/// `POST /v1/leases` body. Extra spec fields are optional so MVP callers can
/// send the subset; unknown keys are ignored so later adapters can grow.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LeaseRequest {
    pub os: Os,
    pub class: Class,
    pub license: License,
    #[serde(default)]
    pub density: Density,
    #[serde(default)]
    pub pooled: bool,
    #[serde(default)]
    pub term: Term,
    pub resources: Resources,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace: Option<Workspace>,
    #[serde(rename = "object", default, skip_serializing_if = "Option::is_none")]
    pub object: Option<ObjectStore>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cpu_overcommit: Option<f64>,
    #[serde(default)]
    pub min_seconds: u64,
    #[serde(default)]
    pub max_seconds: u64,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub exclusive_hardware: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub capabilities: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,
    #[serde(default)]
    pub isolation: Isolation,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub network: Option<Network>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recording: Option<bool>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub human_confirm: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preemptible: Option<bool>,
}

impl LeaseRequest {
    pub fn effective_min_seconds(&self) -> u64 {
        if self.min_seconds == 0 {
            default_min_seconds(self.os, self.density)
        } else {
            self.min_seconds
        }
    }

    pub fn validate_mvp(&self) -> Result<(), MvpError> {
        validate_mvp(self)
    }
}

/// Spec on-demand minima. Linux isolated is 300s (VM boot); shared is 60s.
/// Windows and macOS values exist so the helper is total; MVP still rejects those OS.
#[must_use]
pub fn default_min_seconds(os: Os, density: Density) -> u64 {
    match os {
        Os::Linux => match density {
            Density::Shared => 60,
            Density::Isolated | Density::Exclusive => 300,
        },
        Os::Windows => 60,
        Os::Macos => 86_400,
    }
}

/// MVP control-plane gate: private Linux, isolated|shared, on_demand.
pub fn validate_mvp(req: &LeaseRequest) -> Result<(), MvpError> {
    match req.os {
        Os::Linux => {}
        Os::Windows => return Err(MvpError::UnsupportedOs(Os::Windows)),
        Os::Macos => return Err(MvpError::UnsupportedOs(Os::Macos)),
    }
    match req.class {
        Class::Private => {}
        Class::Mesh | Class::LicensedCloud => {
            return Err(MvpError::UnsupportedClass(req.class));
        }
    }
    match req.density {
        Density::Isolated | Density::Shared => {}
        Density::Exclusive => return Err(MvpError::UnsupportedDensity(req.density)),
    }
    match req.term {
        Term::OnDemand => {}
        Term::Monthly | Term::Annual => return Err(MvpError::UnsupportedTerm(req.term)),
    }
    if req.resources.vcpu == 0 || req.resources.mem_gib == 0 {
        return Err(MvpError::InvalidResources);
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MvpError {
    UnsupportedOs(Os),
    UnsupportedClass(Class),
    UnsupportedDensity(Density),
    UnsupportedTerm(Term),
    InvalidResources,
}

impl std::fmt::Display for MvpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsupportedOs(Os::Windows) => {
                write!(
                    f,
                    "Windows is not supported in v0.1; only os=linux is available (Windows guests are not implemented)"
                )
            }
            Self::UnsupportedOs(Os::Macos) => {
                write!(
                    f,
                    "macOS is not supported in v0.1; only os=linux is available (macOS guests are not implemented)"
                )
            }
            Self::UnsupportedOs(Os::Linux) => {
                write!(f, "unsupported os for v0.1")
            }
            Self::UnsupportedClass(Class::Mesh) => write!(
                f,
                "class=mesh is not supported in v0.1; only class=private is available (mesh is not implemented)"
            ),
            Self::UnsupportedClass(class) => write!(
                f,
                "class={} is not supported in v0.1; only class=private is available",
                class.as_str()
            ),
            Self::UnsupportedDensity(density) => write!(
                f,
                "density={} is not supported in v0.1; only density=isolated or density=shared is available",
                density.as_str()
            ),
            Self::UnsupportedTerm(term) => write!(
                f,
                "term={} is not supported in v0.1; only term=on_demand is available",
                term.as_str()
            ),
            Self::InvalidResources => {
                write!(
                    f,
                    "vcpu and mem_gib must be greater than zero (0 is not unlimited)"
                )
            }
        }
    }
}

impl std::error::Error for MvpError {}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Lease {
    pub lease_id: String,
    pub session_id: String,
    pub expires_at: String,
    pub endpoint: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mcp_stdio: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub viewer: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub driver: Option<String>,
    pub quote: Quote,
}
