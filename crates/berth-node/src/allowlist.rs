use std::env;

use berth_protocol::LeaseRequest;

/// Default guest egress hosts. Unset `BERTH_ALLOWLIST` uses this list.
pub const DEFAULT_ALLOWLIST: &str = "github.com,pypi.org,registry.npmjs.org";

/// Parse a comma-separated allowlist.
///
/// * `None` → default hosts
/// * `Some("")` / whitespace → deny-all (no hosts)
/// * anything else → sanitized hosts (invalid tokens dropped)
pub fn parse_allowlist(raw: Option<&str>) -> Vec<String> {
    match raw {
        None => parse_csv(DEFAULT_ALLOWLIST),
        Some(s) => parse_csv(s),
    }
}

fn parse_csv(raw: &str) -> Vec<String> {
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

/// CSV written into the guest `BERTH_ALLOWLIST` env.
///
/// Lease `network.domains` wins (including empty = deny-all). Otherwise
/// `BERTH_ALLOWLIST` on the node, else the default list.
pub fn csv_for_lease(req: &LeaseRequest) -> String {
    match &req.network {
        Some(net) => net
            .domains
            .iter()
            .map(|s| s.trim().to_ascii_lowercase())
            .filter(|s| valid_host(s))
            .collect::<Vec<_>>()
            .join(","),
        None => match env::var("BERTH_ALLOWLIST") {
            Ok(s) => parse_allowlist(Some(&s)).join(","),
            Err(_) => parse_allowlist(None).join(","),
        },
    }
}

/// iptables-restore fragment for guest OUTPUT.
///
/// Empty domains: default-deny including DNS. Loopback (non-DNS) and
/// ESTABLISHED replies stay so the published viewer still works.
pub fn output_ruleset(domains: &[String]) -> String {
    let mut lines = vec![
        "*filter".to_string(),
        ":OUTPUT DROP [0:0]".to_string(),
        "-A OUTPUT -m conntrack --ctstate ESTABLISHED,RELATED -j ACCEPT".to_string(),
    ];
    if domains.is_empty() {
        lines.push("-A OUTPUT -p udp --dport 53 -j DROP".into());
        lines.push("-A OUTPUT -p tcp --dport 53 -j DROP".into());
    }
    lines.push("-A OUTPUT -o lo -j ACCEPT".into());
    if !domains.is_empty() {
        lines.push("# dns udp/tcp 53 to resolv.conf nameservers".into());
        for d in domains {
            lines.push(format!("# tcp 80,443 to resolved IPs of {d}"));
        }
    }
    lines.push("COMMIT".into());
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use berth_protocol::{
        Class, Density, Egress, Isolation, LeaseRequest, License, Network, Os, Resources, Term,
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
    fn parse_none_is_default() {
        assert_eq!(
            parse_allowlist(None),
            vec![
                "github.com".to_string(),
                "pypi.org".to_string(),
                "registry.npmjs.org".to_string()
            ]
        );
    }

    #[test]
    fn parse_empty_is_deny_all() {
        assert!(parse_allowlist(Some("")).is_empty());
        assert!(parse_allowlist(Some("   ")).is_empty());
        assert!(parse_allowlist(Some(",,")).is_empty());
    }

    #[test]
    fn parse_custom_and_sanitize() {
        assert_eq!(
            parse_allowlist(Some("GitHub.COM, pypi.org")),
            vec!["github.com".to_string(), "pypi.org".to_string()]
        );
        assert_eq!(
            parse_allowlist(Some("github.com,,pypi.org,")),
            vec!["github.com".to_string(), "pypi.org".to_string()]
        );
        assert_eq!(
            parse_allowlist(Some("github.com, evil.com;id, pypi.org")),
            vec!["github.com".to_string(), "pypi.org".to_string()]
        );
        assert!(parse_allowlist(Some("https://github.com")).is_empty());
        assert!(parse_allowlist(Some("*")).is_empty());
        assert_eq!(
            parse_allowlist(Some("1.2.3.4")),
            vec!["1.2.3.4".to_string()]
        );
    }

    #[test]
    fn empty_ruleset_is_deny_all() {
        let rs = output_ruleset(&[]);
        assert!(rs.contains(":OUTPUT DROP"), "{rs}");
        assert!(
            rs.contains("-m conntrack --ctstate ESTABLISHED,RELATED -j ACCEPT"),
            "{rs}"
        );
        assert!(rs.contains("-A OUTPUT -o lo -j ACCEPT"), "{rs}");
        assert!(rs.contains("-A OUTPUT -p udp --dport 53 -j DROP"), "{rs}");
        assert!(rs.contains("-A OUTPUT -p tcp --dport 53 -j DROP"), "{rs}");
        assert!(!rs.contains("443"), "{rs}");
        assert!(!rs.contains("80,443"), "{rs}");
        assert!(!rs.contains("github.com"), "{rs}");
        assert!(!rs.contains("nameserver"), "{rs}");
    }

    #[test]
    fn default_ruleset_allows_resolved_https() {
        let rs = output_ruleset(&parse_allowlist(None));
        assert!(rs.contains(":OUTPUT DROP"), "{rs}");
        assert!(rs.contains("github.com"), "{rs}");
        assert!(rs.contains("pypi.org"), "{rs}");
        assert!(rs.contains("registry.npmjs.org"), "{rs}");
        assert!(rs.contains("tcp 80,443"), "{rs}");
        assert!(!rs.contains("--dport 53 -j DROP"), "{rs}");
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
        assert!(
            output_ruleset(&parse_allowlist(Some(&csv_for_lease(&deny))))
                .contains("--dport 53 -j DROP")
        );

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
