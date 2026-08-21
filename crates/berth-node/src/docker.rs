use std::collections::HashMap;
use std::env;

use berth_protocol::Resources;
use bollard::models::{
    ContainerCreateBody, HostConfig, Mount, MountType, PortBinding, VolumeCreateRequest,
};

use crate::error::{Error, Result};

pub const DEFAULT_IMAGE: &str = "berthos-linux-xfce:dev";
pub const WORKSPACE_MOUNT: &str = "/workspace";
pub const VIEWER_PORT: &str = "6080/tcp";
const GIB: i64 = 1 << 30;
const NANO: i64 = 1_000_000_000;

pub fn image_from_env() -> String {
    env::var("BERTH_IMAGE")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| DEFAULT_IMAGE.to_string())
}

/// Named volume `berth-ws-<workspace_id>` mounted at `/workspace`.
pub fn volume_name(workspace_id: &str) -> String {
    let mut body = String::with_capacity(workspace_id.len());
    for c in workspace_id.chars() {
        if c.is_ascii_alphanumeric() || c == '_' || c == '.' || c == '-' {
            body.push(c);
        } else {
            body.push('-');
        }
    }
    if body.len() > 180 {
        body.truncate(180);
    }
    if body.is_empty() {
        body.push_str("default");
    }
    format!("berth-ws-{body}")
}

pub fn container_name(session_id: &str) -> String {
    let mut name = format!("berth-{session_id}");
    name.retain(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.' || c == '-');
    if name.len() > 120 {
        name.truncate(120);
    }
    name
}

pub fn host_config(resources: &Resources, volume: &str) -> Result<HostConfig> {
    let memory = if resources.mem_gib == 0 {
        None
    } else {
        Some(
            i64::from(resources.mem_gib)
                .checked_mul(GIB)
                .ok_or(Error::ResourceOverflow("mem_gib"))?,
        )
    };
    let nano_cpus = if resources.vcpu == 0 {
        None
    } else {
        Some(
            i64::from(resources.vcpu)
                .checked_mul(NANO)
                .ok_or(Error::ResourceOverflow("vcpu"))?,
        )
    };

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
        memory,
        nano_cpus,
        // Isolated guest network. Never `host` — that is the host desktop path.
        network_mode: Some("bridge".into()),
        privileged: Some(false),
        cap_drop: Some(vec!["ALL".into()]),
        security_opt: Some(vec!["no-new-privileges:true".into()]),
        auto_remove: Some(false),
        // Named volume only. Never bind /tmp/.X11-unix or a host display.
        binds: None,
        mounts: Some(vec![Mount {
            target: Some(WORKSPACE_MOUNT.into()),
            source: Some(volume.to_string()),
            typ: Some(MountType::VOLUME),
            read_only: Some(false),
            ..Default::default()
        }]),
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

pub fn container_body(
    image: &str,
    session_id: &str,
    workspace_id: &str,
    resources: &Resources,
    volume: &str,
) -> Result<ContainerCreateBody> {
    let host_config = host_config(resources, volume)?;
    let mut labels = HashMap::new();
    labels.insert("berth.session_id".into(), session_id.to_string());
    labels.insert("berth.workspace_id".into(), workspace_id.to_string());
    Ok(ContainerCreateBody {
        image: Some(image.to_string()),
        hostname: Some("berth".into()),
        working_dir: Some(WORKSPACE_MOUNT.into()),
        // Pin guest DISPLAY. Do not inherit a host XQuartz/X11 value.
        env: Some(vec![
            "DISPLAY=:99".into(),
            "WIDTH=1280".into(),
            "HEIGHT=800".into(),
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

#[cfg(test)]
mod tests {
    use super::*;
    use berth_protocol::Resources;

    fn sample() -> HostConfig {
        host_config(
            &Resources {
                vcpu: 2,
                mem_gib: 4,
                disk_gib: 40,
            },
            "berth-ws-abc",
        )
        .unwrap()
    }

    #[test]
    fn volume_and_image_names() {
        assert_eq!(volume_name("abc"), "berth-ws-abc");
        assert_eq!(volume_name("ws_foo/bar"), "berth-ws-ws_foo-bar");
        assert_eq!(volume_name(""), "berth-ws-default");
        assert_eq!(image_from_env(), DEFAULT_IMAGE);
        assert!(container_name("s_deadbeef").starts_with("berth-s_"));
    }

    #[test]
    fn never_host_net_or_host_display() {
        let host = sample();
        assert_eq!(host.network_mode.as_deref(), Some("bridge"));
        assert_eq!(host.privileged, Some(false));
        assert_eq!(
            host.cap_drop.as_deref(),
            Some(["ALL".to_string()].as_slice())
        );
        assert!(host.binds.is_none());
        let mounts = host.mounts.as_ref().expect("mounts");
        assert_eq!(mounts.len(), 1);
        assert_eq!(mounts[0].target.as_deref(), Some(WORKSPACE_MOUNT));
        assert_eq!(mounts[0].source.as_deref(), Some("berth-ws-abc"));
        assert_eq!(mounts[0].typ, Some(MountType::VOLUME));
        let dump = format!("{host:?}");
        assert!(!dump.contains("/tmp/.X11-unix"));
        assert!(!dump.contains("DISPLAY="));
        assert_eq!(host.memory, Some(4 * GIB));
        assert_eq!(host.nano_cpus, Some(2 * NANO));
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
            "berth-ws-ws_1",
        )
        .unwrap();
        let env = body.env.expect("env");
        assert!(env.iter().any(|e| e == "DISPLAY=:99"));
        assert!(!env.iter().any(|e| e.contains("/tmp/.X11-unix")));
        assert_eq!(
            body.host_config
                .as_ref()
                .and_then(|h| h.network_mode.as_deref()),
            Some("bridge")
        );
    }
}
