use serde::{Deserialize, Serialize};

use crate::lease::{Density, LeaseRequest, MvpError, Os, Term, validate_mvp};

/// Seed USD / vCPU-second from `docs/MATH.md`.
pub const P_CPU: f64 = 0.000_003_5;
/// Seed USD / GiB-RAM-second from `docs/MATH.md`.
pub const P_MEM: f64 = 0.000_001_1;
/// Seed USD / GiB-disk-second from `docs/MATH.md`.
pub const P_DISK: f64 = 0.000_000_05;
/// Shared sessions are overcommit; MATH.md sets this at 0.30 of isolated.
pub const DENSITY_MULT_SHARED: f64 = 0.30;
pub const DENSITY_MULT_ISOLATED: f64 = 1.0;
pub const OS_MULT_LINUX: f64 = 1.0;
/// Spec example peg. Internal meter is gas; user-facing is USD.
pub const USD_PER_GAS: f64 = 0.01;
pub const CURRENCY_GAS: &str = "gas";

#[must_use]
pub fn density_mult(density: Density) -> f64 {
    match density {
        Density::Shared => DENSITY_MULT_SHARED,
        // Exclusive is a whole-box quote on the mesh, not a slice multiplier.
        Density::Isolated | Density::Exclusive => DENSITY_MULT_ISOLATED,
    }
}

#[must_use]
pub fn os_mult(os: Os) -> f64 {
    match os {
        Os::Linux => OS_MULT_LINUX,
        // Cloud list prices already include the OS; do not invent 1.5× / 2.0×.
        Os::Windows | Os::Macos => OS_MULT_LINUX,
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Quote {
    pub vcpu: u32,
    pub mem_gib: u32,
    pub disk_gib: u32,
    pub os: Os,
    pub os_mult: f64,
    pub density: Density,
    pub density_mult: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub term: Option<Term>,
    pub min_seconds: u64,
    pub pooled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cpu_overcommit: Option<f64>,
    pub gas_per_second: String,
    pub currency: String,
    pub usd_per_gas: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub preemptible: bool,
}

impl Quote {
    /// Wall-clock on-demand quote from MATH.md seed prices. No protocol cut
    /// (that is a mesh settlement concern; MVP only prints the meter).
    ///
    /// Refuses anything `validate_mvp` rejects so licensed-cloud / mesh / non-Linux
    /// requests cannot pick up the private Linux seed formula (and so `os_mult` is
    /// never applied on a licensed-cloud quote).
    pub fn from_request(req: &LeaseRequest) -> Result<Self, MvpError> {
        validate_mvp(req)?;
        let os_mult = os_mult(req.os);
        let density_mult = density_mult(req.density);
        let usd_per_second = (P_CPU * f64::from(req.resources.vcpu)
            + P_MEM * f64::from(req.resources.mem_gib)
            + P_DISK * f64::from(req.resources.disk_gib))
            * density_mult
            * os_mult;
        let gas_per_second = usd_per_second / USD_PER_GAS;
        Ok(Self {
            vcpu: req.resources.vcpu,
            mem_gib: req.resources.mem_gib,
            disk_gib: req.resources.disk_gib,
            os: req.os,
            os_mult,
            density: req.density,
            density_mult,
            term: Some(req.term),
            min_seconds: req.effective_min_seconds(),
            pooled: req.pooled,
            cpu_overcommit: req.cpu_overcommit,
            gas_per_second: format_decimal(gas_per_second),
            currency: CURRENCY_GAS.into(),
            usd_per_gas: format_decimal(USD_PER_GAS),
            preemptible: req.preemptible.unwrap_or(false),
        })
    }

    pub fn usd_per_second(&self) -> Result<f64, std::num::ParseFloatError> {
        let gas: f64 = self.gas_per_second.parse()?;
        let usd_per_gas: f64 = self.usd_per_gas.parse()?;
        Ok(gas * usd_per_gas)
    }
}

fn format_decimal(v: f64) -> String {
    if v == 0.0 {
        return "0".into();
    }
    let s = format!("{v:.12}");
    let trimmed = s.trim_end_matches('0').trim_end_matches('.');
    if trimmed.is_empty() || trimmed == "-" {
        "0".into()
    } else if let Some(rest) = trimmed.strip_prefix('.') {
        format!("0.{rest}")
    } else if let Some(rest) = trimmed.strip_prefix("-.") {
        format!("-0.{rest}")
    } else {
        trimmed.to_string()
    }
}
