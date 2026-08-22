use std::collections::HashMap;
use std::env;

use berthos_protocol::Resources;
use bollard::models::{
    ContainerCreateBody, HostConfig, Mount, MountType, NetworkCreateRequest, PortBinding,
    VolumeCreateRequest,
};
use sha2::{Digest, Sha256};

use crate::error::{Error, Result};
use crate::id::hex_lower;

pub const DEFAULT_IMAGE: &str = "berthos-linux-xfce:dev";
pub const WORKSPACE_MOUNT: &str = "/workspace";
/// Staged by the node before the guest starts and synced back after it stops,
/// so the guest sees ordinary local files and never the bucket credentials.
pub const OBJECT_MOUNT: &str = "/mnt/s3";
pub const VIEWER_PORT: &str = "6080/tcp";
const GIB: i64 = 1 << 30;
const NANO: i64 = 1_000_000_000;
const NAME_MAX: usize = 200;
/// Caps to install the OUTPUT allowlist, drop to `berth`, and clear the bounding set.
const GUEST_CAPS: [&str; 4] = ["NET_ADMIN", "SETUID", "SETGID", "SETPCAP"];

pub fn resolve_image(env_val: Option<&str>) -> String {
    env_val
        .filter(|s| !s.is_empty())
        .unwrap_or(DEFAULT_IMAGE)
        .to_string()
}

/// Label stamped by `images/linux-xfce/Dockerfile` proving the image applies an
/// egress filter. Existence of an image says nothing about what is inside it, so
/// the health probe requires this instead.
pub const EGRESS_LABEL: &str = "berth.egress.version";

/// Egress contract the node understands. Bump alongside the Dockerfile when the
/// filter's guarantees change so stale images stop reporting healthy.
///
/// v1 filtered addresses only, so a guest could still resolve any name and use
/// DNS as an exfiltration channel. v2 filters names as well.
pub const EGRESS_VERSION: &str = "2";

pub fn image_from_env() -> String {
    resolve_image(env::var("BERTH_IMAGE").ok().as_deref())
}

/// Named volume `berth-s3-<workspace_id>` at `/mnt/s3`.
///
/// Separate from the workspace volume so a bucket can be attached and detached
/// without touching files the agent owns.
pub fn s3_volume_name(workspace_id: &str) -> String {
    namespaced("berth-s3-", workspace_id)
}

/// Named volume `berth-ws-<workspace_id>` at `/workspace`.
///
/// Valid Docker suffixes are used as-is. Anything else gets a sanitized body
/// plus a sha256 prefix so two ids cannot alias the same volume.
pub fn volume_name(workspace_id: &str) -> String {
    namespaced("berth-ws-", workspace_id)
}

pub fn container_name(session_id: &str) -> String {
    namespaced("berth-", session_id)
}

pub fn network_name(session_id: &str) -> String {
    namespaced("berth-net-", session_id)
}

fn namespaced(prefix: &str, id: &str) -> String {
    let candidate = format!("{prefix}{id}");
    if is_docker_name_suffix(id) && is_docker_name_suffix(&candidate) && candidate.len() <= NAME_MAX
    {
        return candidate;
    }
    let tag = sha8(id.as_bytes());
    let mut safe: String = id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '.' || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect();
    let budget = NAME_MAX.saturating_sub(prefix.len() + 1 + tag.len());
    if safe.len() > budget {
        safe.truncate(budget);
    }
    if safe.is_empty() {
        format!("{prefix}x{tag}")
    } else {
        format!("{prefix}{safe}-{tag}")
    }
}

fn is_docker_name_suffix(s: &str) -> bool {
    if s.is_empty() || s.len() > NAME_MAX {
        return false;
    }
    let mut chars = s.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    first.is_ascii_alphanumeric()
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.' || c == '-')
}

fn sha8(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    hex_lower(&digest[..4])
}

pub fn host_config(
    resources: &Resources,
    volume: &str,
    network: &str,
    object_volume: Option<&str>,
) -> Result<HostConfig> {
    if resources.vcpu == 0 || resources.mem_gib == 0 {
        return Err(Error::InvalidResources);
    }
    if network.eq_ignore_ascii_case("host") {
        return Err(Error::Guest("refusing --network=host".into()));
    }
    let memory = i64::from(resources.mem_gib)
        .checked_mul(GIB)
        .ok_or(Error::ResourceOverflow("mem_gib"))?;
    let nano_cpus = i64::from(resources.vcpu)
        .checked_mul(NANO)
        .ok_or(Error::ResourceOverflow("vcpu"))?;

    let mut port_bindings = HashMap::new();
    port_bindings.insert(
        VIEWER_PORT.to_string(),
        Some(vec![PortBinding {
            // Loopback only; never publish on the host LAN by accident.
            host_ip: Some("127.0.0.1".into()),
            host_port: None,
        }]),
    );

    Ok(HostConfig {
        memory: Some(memory),
        nano_cpus: Some(nano_cpus),
        // Per-session user-defined bridge. Never `host`.
        network_mode: Some(network.to_string()),
        privileged: Some(false),
        cap_drop: Some(vec!["ALL".into()]),
        cap_add: Some(GUEST_CAPS.iter().map(|c| (*c).to_string()).collect()),
        security_opt: Some(vec!["no-new-privileges:true".into()]),
        auto_remove: Some(false),
        // Named volume only. Never bind /tmp/.X11-unix or a host display.
        binds: None,
        mounts: Some(guest_mounts(volume, object_volume)),
        port_bindings: Some(port_bindings),
        shm_size: Some(64 * 1024 * 1024),
        pid_mode: None,
        ipc_mode: None,
        uts_mode: None,
        userns_mode: None,
        extra_hosts: None,
        ..Default::default()
    })
}

fn guest_mounts(volume: &str, object_volume: Option<&str>) -> Vec<Mount> {
    let mut mounts = vec![Mount {
        target: Some(WORKSPACE_MOUNT.into()),
        source: Some(volume.to_string()),
        typ: Some(MountType::VOLUME),
        read_only: Some(false),
        ..Default::default()
    }];
    if let Some(object) = object_volume {
        mounts.push(Mount {
            target: Some(OBJECT_MOUNT.into()),
            source: Some(object.to_string()),
            typ: Some(MountType::VOLUME),
            read_only: Some(false),
            ..Default::default()
        });
    }
    mounts
}

/// The guest's two volumes: its workspace, and an optional staged bucket.
#[derive(Debug, Clone, Copy)]
pub struct GuestVolumes<'a> {
    pub workspace: &'a str,
    pub object: Option<&'a str>,
}

pub fn container_body(
    image: &str,
    session_id: &str,
    workspace_id: &str,
    resources: &Resources,
    volumes: GuestVolumes<'_>,
    network: &str,
    allowlist: &str,
) -> Result<ContainerCreateBody> {
    let host_config = host_config(resources, volumes.workspace, network, volumes.object)?;
    let mut labels = HashMap::new();
    labels.insert("berth.session_id".into(), session_id.to_string());
    labels.insert("berth.workspace_id".into(), workspace_id.to_string());
    Ok(ContainerCreateBody {
        image: Some(image.to_string()),
        hostname: Some("berth".into()),
        working_dir: Some(WORKSPACE_MOUNT.into()),
        // Root applies iptables then drops to berth. Exec uses user berth.
        user: Some("0:0".into()),
        // Pin guest DISPLAY. Do not inherit a host XQuartz/X11 value.
        // Empty BERTH_ALLOWLIST is deny-all (distinct from unset → default).
        env: Some(vec![
            "DISPLAY=:99".into(),
            "WIDTH=1280".into(),
            "HEIGHT=800".into(),
            format!("BERTH_ALLOWLIST={allowlist}"),
        ]),
        exposed_ports: Some(vec![VIEWER_PORT.into()]),
        labels: Some(labels),
        host_config: Some(host_config),
        ..Default::default()
    })
}

pub fn volume_create(name: &str) -> VolumeCreateRequest {
    VolumeCreateRequest {
        name: Some(name.to_string()),
        labels: Some(HashMap::from([("berth.workspace".into(), "1".into())])),
        ..Default::default()
    }
}

pub fn session_network_create(name: &str, session_id: &str) -> NetworkCreateRequest {
    NetworkCreateRequest {
        name: name.to_string(),
        driver: Some("bridge".into()),
        internal: Some(false),
        labels: Some(HashMap::from([(
            "berth.session_id".into(),
            session_id.to_string(),
        )])),
        ..Default::default()
    }
}

pub fn assert_host_isolated(host: &HostConfig) {
    let mode = host.network_mode.as_deref().unwrap_or("");
    assert!(!mode.eq_ignore_ascii_case("host"), "network_mode={mode}");
    assert_eq!(host.privileged, Some(false));
    assert_eq!(
        host.cap_drop.as_deref(),
        Some(["ALL".to_string()].as_slice())
    );
    let add = host.cap_add.as_deref().expect("cap_add");
    assert!(
        add.iter().any(|c| c.eq_ignore_ascii_case("NET_ADMIN")),
        "{add:?}"
    );
    assert!(
        add.iter().any(|c| c.eq_ignore_ascii_case("SETPCAP")),
        "{add:?}"
    );
    for c in add {
        let upper = c.to_ascii_uppercase();
        assert!(
            GUEST_CAPS.iter().any(|ok| ok.eq_ignore_ascii_case(&upper)),
            "unexpected cap {c}"
        );
        assert!(!upper.contains("SYS_ADMIN"));
    }
    let sec = host.security_opt.as_ref().expect("security_opt");
    assert!(
        sec.iter().any(|s| s.contains("no-new-privileges")),
        "{sec:?}"
    );
    assert!(host.binds.as_ref().is_none_or(Vec::is_empty));
    let bindings = host
        .port_bindings
        .as_ref()
        .and_then(|m| m.get(VIEWER_PORT))
        .and_then(|b| b.as_ref())
        .expect("6080 binding");
    assert_eq!(bindings[0].host_ip.as_deref(), Some("127.0.0.1"));
    let dump = format!("{host:?}");
    assert!(!dump.contains("/tmp/.X11-unix"));
}

#[cfg(test)]
mod tests {
    use super::*;
    use berthos_protocol::Resources;

    fn sample() -> HostConfig {
        host_config(
            &Resources {
                vcpu: 2,
                mem_gib: 4,
                disk_gib: 40,
            },
            "berth-ws-abc",
            "berth-net-s_1",
            None,
        )
        .unwrap()
    }

    #[test]
    fn object_mount_appears_only_when_a_bucket_is_attached() {
        let res = Resources {
            vcpu: 1,
            mem_gib: 1,
            disk_gib: 1,
        };
        let targets = |hc: &HostConfig| -> Vec<String> {
            hc.mounts
                .as_ref()
                .expect("mounts")
                .iter()
                .filter_map(|m| m.target.clone())
                .collect()
        };

        let plain = host_config(&res, "berth-ws-a", "berth-net-s_1", None).unwrap();
        assert_eq!(targets(&plain), vec![WORKSPACE_MOUNT.to_string()]);

        let with_bucket =
            host_config(&res, "berth-ws-a", "berth-net-s_1", Some("berth-s3-a")).unwrap();
        assert_eq!(
            targets(&with_bucket),
            vec![WORKSPACE_MOUNT.to_string(), OBJECT_MOUNT.to_string()]
        );
        // Attaching a bucket must not quietly relax the isolation posture.
        assert_host_isolated(&with_bucket);
        assert_eq!(
            with_bucket.cap_drop.as_deref(),
            Some(&["ALL".to_string()][..])
        );
    }

    #[test]
    fn workspace_and_bucket_volumes_never_collide() {
        assert_ne!(volume_name("ws_1"), s3_volume_name("ws_1"));
        assert!(s3_volume_name("ws_1").starts_with("berth-s3-"));
    }

    #[test]
    fn volume_names_do_not_alias() {
        assert_eq!(volume_name("abc"), "berth-ws-abc");
        assert_eq!(volume_name("ws_foo-bar"), "berth-ws-ws_foo-bar");
        let slash = volume_name("ws_foo/bar");
        assert_ne!(slash, volume_name("ws_foo-bar"));
        assert!(slash.starts_with("berth-ws-"));
        assert_ne!(volume_name(""), volume_name("---"));
        assert_ne!(volume_name(""), "berth-ws-default");
        assert!(container_name("s_deadbeef").starts_with("berth-s_"));
        assert!(network_name("s_deadbeef").starts_with("berth-net-s_"));
    }

    #[test]
    fn resolve_image_ignores_empty_env() {
        assert_eq!(resolve_image(None), DEFAULT_IMAGE);
        assert_eq!(resolve_image(Some("")), DEFAULT_IMAGE);
        assert_eq!(resolve_image(Some("custom:tag")), "custom:tag");
    }

    #[test]
    fn never_host_net_or_host_display() {
        let host = sample();
        assert_eq!(host.network_mode.as_deref(), Some("berth-net-s_1"));
        assert_host_isolated(&host);
        let mounts = host.mounts.as_ref().expect("mounts");
        assert_eq!(mounts.len(), 1);
        assert_eq!(mounts[0].target.as_deref(), Some(WORKSPACE_MOUNT));
        assert_eq!(mounts[0].source.as_deref(), Some("berth-ws-abc"));
        assert_eq!(mounts[0].typ, Some(MountType::VOLUME));
        assert_eq!(host.memory, Some(4 * GIB));
        assert_eq!(host.nano_cpus, Some(2 * NANO));
    }

    #[test]
    fn zero_resources_are_rejected() {
        let vol = "berth-ws-abc";
        let net = "berth-net-s_1";
        assert!(
            host_config(
                &Resources {
                    vcpu: 0,
                    mem_gib: 4,
                    disk_gib: 1,
                },
                vol,
                net,
                None,
            )
            .is_err()
        );
        assert!(
            host_config(
                &Resources {
                    vcpu: 2,
                    mem_gib: 0,
                    disk_gib: 1,
                },
                vol,
                net,
                None,
            )
            .is_err()
        );
        assert!(
            host_config(
                &Resources {
                    vcpu: 1,
                    mem_gib: 1,
                    disk_gib: 1,
                },
                vol,
                "host",
                None,
            )
            .is_err()
        );
    }

    #[test]
    fn body_pins_guest_display() {
        let body = container_body(
            DEFAULT_IMAGE,
            "s_1",
            "ws_1",
            &Resources {
                vcpu: 1,
                mem_gib: 1,
                disk_gib: 1,
            },
            GuestVolumes {
                workspace: "berth-ws-ws_1",
                object: None,
            },
            "berth-net-s_1",
            berthos_protocol::DEFAULT_ALLOWLIST,
        )
        .unwrap();
        let env = body.env.expect("env");
        assert!(env.iter().any(|e| e == "DISPLAY=:99"));
        assert!(!env.iter().any(|e| e.contains("/tmp/.X11-unix")));
        assert!(
            env.iter()
                .any(|e| e == "BERTH_ALLOWLIST=github.com,pypi.org,registry.npmjs.org")
        );
        assert_eq!(body.user.as_deref(), Some("0:0"));
        let host = body.host_config.as_ref().expect("host");
        assert_host_isolated(host);
        assert_eq!(host.network_mode.as_deref(), Some("berth-net-s_1"));
    }

    #[test]
    fn empty_allowlist_env_is_set_not_omitted() {
        let body = container_body(
            DEFAULT_IMAGE,
            "s_1",
            "ws_1",
            &Resources {
                vcpu: 1,
                mem_gib: 1,
                disk_gib: 1,
            },
            GuestVolumes {
                workspace: "berth-ws-ws_1",
                object: None,
            },
            "berth-net-s_1",
            "",
        )
        .unwrap();
        let env = body.env.expect("env");
        assert!(env.iter().any(|e| e == "BERTH_ALLOWLIST="));
        assert!(!env.iter().any(|e| e == "BERTH_ALLOWLIST"));
    }
}
