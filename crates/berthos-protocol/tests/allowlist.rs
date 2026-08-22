use berthos_protocol::{DEFAULT_ALLOWLIST, network_from_allowlist_key, parse_allowlist};

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
fn missing_config_key_omits_network() {
    assert!(network_from_allowlist_key(None).is_none());
}

#[test]
fn present_empty_key_is_deny_all_network() {
    let net = network_from_allowlist_key(Some("")).expect("network");
    assert!(net.domains.is_empty());
}

#[test]
fn present_key_sends_sanitized_domains() {
    let net = network_from_allowlist_key(Some(DEFAULT_ALLOWLIST)).expect("network");
    assert_eq!(
        net.domains,
        vec![
            "github.com".to_string(),
            "pypi.org".to_string(),
            "registry.npmjs.org".to_string()
        ]
    );
}
