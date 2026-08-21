use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::config::Config;
use crate::error::{Error, Result};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Status {
    Ok,
    Warn,
    Fail,
}

#[derive(Debug)]
struct Line {
    status: Status,
    detail: String,
}

#[derive(Debug)]
pub(crate) struct DoctorReport {
    lines: Vec<Line>,
}

impl DoctorReport {
    fn failed(&self) -> bool {
        self.lines.iter().any(|l| l.status == Status::Fail)
    }

    fn render(&self) -> String {
        self.lines
            .iter()
            .map(|l| {
                let tag = match l.status {
                    Status::Ok => "ok  ",
                    Status::Warn => "warn",
                    Status::Fail => "fail",
                };
                format!("{tag}  {}", l.detail)
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

pub(crate) struct DoctorProbes {
    pub docker: fn() -> std::result::Result<String, String>,
    pub image: fn(&str) -> std::result::Result<String, String>,
    pub cloudflared: fn() -> Option<String>,
}

impl DoctorProbes {
    fn live() -> Self {
        Self {
            docker: probe_docker,
            image: probe_image,
            cloudflared: probe_cloudflared,
        }
    }
}

pub(crate) fn run_doctor(home: &Path) -> Result<String> {
    let image = berth_node::image_from_env();
    let report = doctor_report(home, &image, &DoctorProbes::live());
    let out = report.render();
    if report.failed() {
        Err(Error::Doctor(out))
    } else {
        Ok(out)
    }
}

pub(crate) fn doctor_report(home: &Path, image: &str, probes: &DoctorProbes) -> DoctorReport {
    let mut lines = Vec::new();

    match (probes.docker)() {
        Ok(detail) => lines.push(Line {
            status: Status::Ok,
            detail,
        }),
        Err(detail) => lines.push(Line {
            status: Status::Fail,
            detail,
        }),
    }

    match (probes.image)(image) {
        Ok(detail) => lines.push(Line {
            status: Status::Ok,
            detail,
        }),
        Err(detail) => lines.push(Line {
            status: Status::Fail,
            detail,
        }),
    }

    match probe_home(home) {
        Ok(detail) => lines.push(Line {
            status: Status::Ok,
            detail,
        }),
        Err(detail) => lines.push(Line {
            status: Status::Fail,
            detail,
        }),
    }

    lines.push(probe_pair(home));

    match (probes.cloudflared)() {
        Some(path) => lines.push(Line {
            status: Status::Ok,
            detail: format!("cloudflared {path}"),
        }),
        None => lines.push(Line {
            status: Status::Warn,
            detail:
                "cloudflared not on PATH (optional; needed for berth node up --tunnel cloudflare)"
                    .into(),
        }),
    }

    lines.push(Line {
        status: Status::Ok,
        detail: "host desktop is never driven".into(),
    });

    DoctorReport { lines }
}

fn probe_docker() -> std::result::Result<String, String> {
    match Command::new("docker")
        .args(["info", "-f", "{{.ServerVersion}}"])
        .output()
    {
        Ok(out) if out.status.success() => {
            let v = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if v.is_empty() {
                Ok("docker".into())
            } else {
                Ok(format!("docker {v}"))
            }
        }
        Ok(out) => {
            let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
            if err.is_empty() {
                Err("docker not reachable".into())
            } else {
                Err(format!("docker not reachable: {err}"))
            }
        }
        Err(_) => Err("docker not reachable; install Docker Desktop or OrbStack".into()),
    }
}

fn probe_image(name: &str) -> std::result::Result<String, String> {
    match Command::new("docker")
        .args(["image", "inspect", name])
        .output()
    {
        Ok(out) if out.status.success() => Ok(format!("image {name}")),
        _ => Err(format!(
            "image {name} not found; docker build -t {name} images/linux-xfce"
        )),
    }
}

fn probe_cloudflared() -> Option<String> {
    if let Some(path) = std::env::var_os("BERTH_CLOUDFLARED") {
        let path = PathBuf::from(path);
        return path.is_file().then(|| path.display().to_string());
    }
    find_on_path("cloudflared").map(|p| p.display().to_string())
}

fn find_on_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).find_map(|dir| {
        let candidate = dir.join(name);
        candidate.is_file().then_some(candidate)
    })
}

fn probe_home(home: &Path) -> std::result::Result<String, String> {
    fs::create_dir_all(home)
        .map_err(|e| format!("BERTH_HOME {} not writable: {e}", home.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(home)
            .map_err(|e| format!("BERTH_HOME {} not writable: {e}", home.display()))?
            .permissions();
        perms.set_mode(0o700);
        fs::set_permissions(home, perms)
            .map_err(|e| format!("BERTH_HOME {} not writable: {e}", home.display()))?;
    }
    let probe = home.join(format!(".doctor-write.{}", std::process::id()));
    write_probe(&probe).map_err(|e| {
        let _ = fs::remove_file(&probe);
        format!("BERTH_HOME {} not writable: {e}", home.display())
    })?;
    let _ = fs::remove_file(&probe);
    Ok(format!("BERTH_HOME {} writable", home.display()))
}

fn write_probe(path: &Path) -> std::io::Result<()> {
    let mut opts = OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut file: File = opts.open(path)?;
    file.write_all(b"ok\n")?;
    file.sync_all()
}

fn probe_pair(home: &Path) -> Line {
    match Config::load(home) {
        Err(err) => Line {
            status: Status::Fail,
            detail: format!("config.toml: {err}"),
        },
        Ok(cfg) if cfg.nodes.is_empty() => Line {
            status: Status::Warn,
            detail: "no paired node; run berth pair --url http://127.0.0.1:7432 --code <code>"
                .into(),
        },
        Ok(cfg) => {
            let mut names = Vec::new();
            for (name, node) in &cfg.nodes {
                if node.token.trim().is_empty() {
                    return Line {
                        status: Status::Fail,
                        detail: format!("node {name} has no token; run berth pair"),
                    };
                }
                names.push(name.as_str());
            }
            let label = if names.len() == 1 {
                "paired node"
            } else {
                "paired nodes"
            };
            Line {
                status: Status::Ok,
                detail: format!("{label} {}", names.join(", ")),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::NodeConfig;

    fn docker_ok() -> std::result::Result<String, String> {
        Ok("docker 27.0".into())
    }
    fn docker_fail() -> std::result::Result<String, String> {
        Err("docker not reachable; install Docker Desktop or OrbStack".into())
    }
    fn image_ok(_: &str) -> std::result::Result<String, String> {
        Ok("image berthos-linux-xfce:dev".into())
    }
    fn image_fail(name: &str) -> std::result::Result<String, String> {
        Err(format!("image {name} not found"))
    }
    fn no_cf() -> Option<String> {
        None
    }
    fn cf_ok() -> Option<String> {
        Some("/usr/local/bin/cloudflared".into())
    }

    fn ok_probes() -> DoctorProbes {
        DoctorProbes {
            docker: docker_ok,
            image: image_ok,
            cloudflared: no_cf,
        }
    }

    #[test]
    fn unpaired_is_warn_and_exit_ok() {
        let dir = tempfile::tempdir().unwrap();
        let report = doctor_report(dir.path(), "berthos-linux-xfce:dev", &ok_probes());
        let out = report.render();
        assert!(!report.failed(), "{out}");
        assert!(out.contains("warn  no paired node"), "{out}");
        assert!(out.contains("ok    host desktop is never driven"), "{out}");
        assert!(out.contains("ok    docker 27.0"), "{out}");
        assert!(out.contains("cloudflared not on PATH"), "{out}");
        assert!(!out.contains("brt_"), "{out}");
    }

    #[test]
    fn paired_node_ok_does_not_print_token() {
        let dir = tempfile::tempdir().unwrap();
        let mut cfg = Config::default();
        cfg.nodes.insert(
            "home-nuc".into(),
            NodeConfig {
                url: "http://127.0.0.1:7432".into(),
                token: "brt_secret".into(),
            },
        );
        cfg.save(dir.path()).unwrap();
        let probes = DoctorProbes {
            docker: docker_ok,
            image: image_ok,
            cloudflared: cf_ok,
        };
        let report = doctor_report(dir.path(), "berthos-linux-xfce:dev", &probes);
        let out = report.render();
        assert!(!report.failed(), "{out}");
        assert!(out.contains("ok    paired node home-nuc"), "{out}");
        assert!(
            out.contains("ok    cloudflared /usr/local/bin/cloudflared"),
            "{out}"
        );
        assert!(!out.contains("brt_secret"), "{out}");
    }

    #[test]
    fn empty_token_fails() {
        let dir = tempfile::tempdir().unwrap();
        let mut cfg = Config::default();
        cfg.nodes.insert(
            "default".into(),
            NodeConfig {
                url: "http://127.0.0.1:7432".into(),
                token: "  ".into(),
            },
        );
        cfg.save(dir.path()).unwrap();
        let report = doctor_report(dir.path(), "berthos-linux-xfce:dev", &ok_probes());
        assert!(report.failed());
        let out = report.render();
        assert!(out.contains("fail  node default has no token"), "{out}");
        assert!(!out.contains("brt_"), "{out}");
    }

    #[test]
    fn docker_or_image_fail_is_nonzero() {
        let dir = tempfile::tempdir().unwrap();
        let report = doctor_report(
            dir.path(),
            "berthos-linux-xfce:dev",
            &DoctorProbes {
                docker: docker_fail,
                image: image_ok,
                cloudflared: no_cf,
            },
        );
        assert!(report.failed());
        assert!(report.render().contains("fail  docker not reachable"));

        let report = doctor_report(
            dir.path(),
            "missing:tag",
            &DoctorProbes {
                docker: docker_ok,
                image: image_fail,
                cloudflared: no_cf,
            },
        );
        assert!(report.failed());
        assert!(
            report
                .render()
                .contains("fail  image missing:tag not found")
        );
    }

    #[cfg(unix)]
    #[test]
    fn unwritable_home_fails() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let parent = dir.path().join("locked");
        fs::create_dir(&parent).unwrap();
        let mut perms = fs::metadata(&parent).unwrap().permissions();
        perms.set_mode(0o500);
        fs::set_permissions(&parent, perms).unwrap();
        let home = parent.join("berth");
        let report = doctor_report(&home, "berthos-linux-xfce:dev", &ok_probes());
        let mut perms = fs::metadata(&parent).unwrap().permissions();
        perms.set_mode(0o700);
        fs::set_permissions(&parent, perms).unwrap();
        assert!(report.failed(), "{}", report.render());
        assert!(
            report.render().contains("not writable"),
            "{}",
            report.render()
        );
    }
}
