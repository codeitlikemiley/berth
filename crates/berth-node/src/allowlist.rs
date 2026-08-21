use std::env;

use berth_protocol::{LeaseRequest, parse_allowlist};

/// CSV written into the guest `BERTH_ALLOWLIST` env.
///
/// Lease `network.domains` wins (including empty = deny-all). Otherwise
/// `BERTH_ALLOWLIST` on the node, else the default list.
pub fn csv_for_lease(req: &LeaseRequest) -> String {
    match &req.network {
        Some(net) => parse_allowlist(Some(&net.domains.join(","))).join(","),
        None => match env::var("BERTH_ALLOWLIST") {
            Ok(s) => parse_allowlist(Some(&s)).join(","),
            Err(_) => parse_allowlist(None).join(","),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use berth_protocol::{
        Class, DEFAULT_ALLOWLIST, Density, Egress, Isolation, LeaseRequest, License, Network, Os,
        Resources, Term,
    };
    use std::sync::Mutex;

    static ENV: Mutex<()> = Mutex::new(());

    fn sample_req(network: Option<Network>) -> LeaseRequest {
        LeaseRequest {
            os: Os::Linux,
            class: Class::Private,
            license: License::Linux,
            density: Density::Isolated,
            pooled: false,
            term: Term::OnDemand,
            resources: Resources {
                vcpu: 1,
                mem_gib: 1,
                disk_gib: 1,
            },
            workspace: None,
            object: None,
            cpu_overcommit: None,
            min_seconds: 60,
            max_seconds: 0,
            exclusive_hardware: false,
            capabilities: vec![],
            image: None,
            region: None,
            isolation: Isolation::Vm,
            network,
            recording: None,
            human_confirm: vec![],
            preemptible: None,
        }
    }

    #[test]
    fn lease_domains_override_env() {
        let _lock = ENV.lock().unwrap_or_else(|p| p.into_inner());
        // SAFETY: lock serializes BERTH_ALLOWLIST mutation in these tests.
        unsafe { env::set_var("BERTH_ALLOWLIST", "should.not.use.example") };
        struct Restore;
        impl Drop for Restore {
            fn drop(&mut self) {
                // SAFETY: lock serializes BERTH_ALLOWLIST mutation in these tests.
                unsafe { env::remove_var("BERTH_ALLOWLIST") };
            }
        }
        let _restore = Restore;

        let deny = sample_req(Some(Network {
            egress: Egress::Allowlist,
            domains: vec![],
        }));
        assert_eq!(csv_for_lease(&deny), "");

        let custom = sample_req(Some(Network {
            egress: Egress::Allowlist,
            domains: vec!["pypi.org".into()],
        }));
        assert_eq!(csv_for_lease(&custom), "pypi.org");
    }

    #[test]
    fn unset_env_uses_default_when_lease_has_no_network() {
        let _lock = ENV.lock().unwrap_or_else(|p| p.into_inner());
        // SAFETY: lock serializes BERTH_ALLOWLIST mutation in these tests.
        unsafe { env::remove_var("BERTH_ALLOWLIST") };
        let csv = csv_for_lease(&sample_req(None));
        assert_eq!(csv, DEFAULT_ALLOWLIST);
    }

    #[test]
    fn empty_env_is_deny_all_when_lease_has_no_network() {
        let _lock = ENV.lock().unwrap_or_else(|p| p.into_inner());
        // SAFETY: lock serializes BERTH_ALLOWLIST mutation in these tests.
        unsafe { env::set_var("BERTH_ALLOWLIST", "") };
        struct Restore;
        impl Drop for Restore {
            fn drop(&mut self) {
                // SAFETY: lock serializes BERTH_ALLOWLIST mutation in these tests.
                unsafe { env::remove_var("BERTH_ALLOWLIST") };
            }
        }
        let _restore = Restore;
        assert_eq!(csv_for_lease(&sample_req(None)), "");
    }
}
