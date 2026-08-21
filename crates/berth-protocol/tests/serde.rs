//! Table-driven serde tests against spec/computer-session.md JSON.

use std::fs;
use std::path::PathBuf;

use berth_protocol::{
    Ack, Action, ActionBatch, Button, Class, DENSITY_MULT_ISOLATED, DENSITY_MULT_SHARED, Density,
    Frame, Isolation, Lease, LeaseRequest, License, MvpError, Os, P_CPU, P_DISK, P_MEM, Quote,
    Term, USD_PER_GAS, default_min_seconds, scale_coordinates, validate_mvp,
};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::json;

fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn load_json(name: &str) -> serde_json::Value {
    let raw = fs::read_to_string(fixture_path(name)).unwrap_or_else(|e| panic!("{name}: {e}"));
    serde_json::from_str(&raw).unwrap_or_else(|e| panic!("{name}: {e}"))
}

fn load<T: DeserializeOwned>(name: &str) -> T {
    serde_json::from_value(load_json(name)).unwrap_or_else(|e| panic!("{name}: {e}"))
}

fn mvp_lease_json(patch: serde_json::Value) -> serde_json::Value {
    let mut base = json!({
        "os": "linux",
        "class": "private",
        "license": "linux",
        "density": "isolated",
        "pooled": false,
        "term": "on_demand",
        "resources": { "vcpu": 2, "mem_gib": 4, "disk_gib": 40 },
        "min_seconds": 60,
        "max_seconds": 3600
    });
    if let (Some(base_obj), Some(patch_obj)) = (base.as_object_mut(), patch.as_object()) {
        for (k, v) in patch_obj {
            base_obj.insert(k.clone(), v.clone());
        }
    }
    base
}

fn parse_lease(patch: serde_json::Value) -> LeaseRequest {
    serde_json::from_value(mvp_lease_json(patch)).expect("lease json")
}

fn roundtrip<T>(value: &T)
where
    T: Serialize + DeserializeOwned + PartialEq + std::fmt::Debug,
{
    let json = serde_json::to_string(value).expect("serialize");
    let back: T = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(&back, value, "{json}");
}

#[test]
fn spec_fixtures_parse_and_roundtrip() {
    check_action_batch(load_json("action_batch.json"));
    check_ack(load_json("ack.json"));
    check_frame(load_json("frame.json"));
    check_lease_request(load_json("lease_request.json"));
    check_lease_request_mvp(load_json("lease_request_mvp.json"));
    check_lease(load_json("lease.json"));
    check_quote_fixture(load_json("quote.json"));
}

fn check_action_batch(value: serde_json::Value) {
    let batch: ActionBatch = serde_json::from_value(value).unwrap();
    assert_eq!(batch.id, "a_...");
    assert_eq!(batch.session_id, "s_...");
    let ops: Vec<_> = batch.items.iter().map(Action::op).collect();
    assert_eq!(
        ops,
        [
            "screenshot",
            "click",
            "double_click",
            "move",
            "drag",
            "scroll",
            "type",
            "key",
            "hold_key",
            "wait",
            "zoom",
            "cursor_position",
            "shell",
        ]
    );
    match &batch.items[1] {
        Action::Click { button, xy, mods } => {
            assert_eq!(*button, Button::Left);
            assert_eq!(*xy, [100, 200]);
            assert!(mods.is_empty());
        }
        other => panic!("expected click, got {other:?}"),
    }
    match &batch.items[4] {
        Action::Drag { path } => assert_eq!(path.as_slice(), &[[100, 200], [300, 200]]),
        other => panic!("expected drag, got {other:?}"),
    }
    match &batch.items[7] {
        Action::Key { keys, repeat } => {
            assert_eq!(keys, &["META".to_string(), "s".to_string()]);
            assert_eq!(*repeat, 1);
        }
        other => panic!("expected key, got {other:?}"),
    }
    match &batch.items[12] {
        Action::Shell { cmd } => assert_eq!(cmd, "uname -a"),
        other => panic!("expected shell, got {other:?}"),
    }
    roundtrip(&batch);
}

fn check_ack(value: serde_json::Value) {
    let ack: Ack = serde_json::from_value(value).unwrap();
    assert_eq!(ack.id, "a_...");
    assert!(ack.results[0].ok);
    assert!(ack.results[0].frame);
    assert!(!ack.results[1].ok);
    assert_eq!(
        ack.results[1].error.as_deref(),
        Some("policy:key:Return requires human")
    );
    assert_eq!(ack.results[2].error.as_deref(), Some("skipped"));
    roundtrip(&ack);
}

fn check_frame(value: serde_json::Value) {
    let frame: Frame = serde_json::from_value(value).unwrap();
    assert_eq!(frame.session_id, "s_...");
    assert_eq!(frame.width, 1280);
    assert_eq!(frame.height, 800);
    assert_eq!(frame.mime, "image/png");
    assert_eq!(frame.cursor, Some([640, 400]));
    assert_eq!(&frame.data[..8], b"\x89PNG\r\n\x1a\n");
    roundtrip(&frame);
}

fn check_lease_request(value: serde_json::Value) {
    let req: LeaseRequest = serde_json::from_value(value).unwrap();
    assert_eq!(req.os, Os::Linux);
    assert_eq!(req.class, Class::Private);
    assert_eq!(req.license, License::Linux);
    assert_eq!(req.density, Density::Shared);
    assert!(!req.pooled);
    assert_eq!(req.term, Term::OnDemand);
    assert_eq!(req.resources.vcpu, 2);
    assert_eq!(req.resources.mem_gib, 4);
    assert_eq!(req.resources.disk_gib, 40);
    assert_eq!(req.min_seconds, 300);
    assert_eq!(req.max_seconds, 86_400);
    assert_eq!(req.isolation, Isolation::Vm);
    let net = req.network.as_ref().expect("network");
    assert_eq!(net.domains, ["github.com", "pypi.org"]);
    validate_mvp(&req).unwrap();
    roundtrip(&req);
}

fn check_lease_request_mvp(value: serde_json::Value) {
    let req: LeaseRequest = serde_json::from_value(value).unwrap();
    assert_eq!(req.density, Density::Isolated);
    assert_eq!(req.image.as_deref(), Some("berth/linux-xfce:2026-08"));
    validate_mvp(&req).unwrap();
    roundtrip(&req);
}

fn check_lease(value: serde_json::Value) {
    let lease: Lease = serde_json::from_value(value).unwrap();
    assert_eq!(lease.lease_id, "l_...");
    assert_eq!(lease.session_id, "s_...");
    assert_eq!(lease.quote.gas_per_second, "0.000333");
    assert_eq!(lease.quote.currency, "gas");
    assert_eq!(lease.quote.usd_per_gas, "0.01");
    assert_eq!(lease.quote.density, Density::Shared);
    roundtrip(&lease);
}

fn check_quote_fixture(value: serde_json::Value) {
    let quote: Quote = serde_json::from_value(value).unwrap();
    assert_eq!(quote.os, Os::Linux);
    assert_eq!(quote.density_mult, DENSITY_MULT_SHARED);
    assert_eq!(quote.min_seconds, 60);
    assert!(quote.preemptible);
    roundtrip(&quote);
}

#[test]
fn action_items_deny_unknown_fields() {
    let cases = [
        json!({"op": "screenshot", "extra": true}),
        json!({"op": "click", "button": "left", "xy": [1, 2], "mods": [], "foo": 1}),
        json!({"op": "wait", "ms": 10, "secret": "no"}),
        json!({"op": "explode"}),
    ];
    for value in cases {
        let err = serde_json::from_value::<Action>(value.clone()).unwrap_err();
        assert!(
            err.to_string().contains("unknown field")
                || err.to_string().contains("unknown variant"),
            "expected unknown field/variant for {value}, got {err}"
        );
    }
}

#[test]
fn click_defaults_button_and_repeat() {
    let click: Action = serde_json::from_value(json!({"op": "click", "xy": [1, 2]})).unwrap();
    match click {
        Action::Click { button, mods, .. } => {
            assert_eq!(button, Button::Left);
            assert!(mods.is_empty());
        }
        other => panic!("{other:?}"),
    }
    let key: Action = serde_json::from_value(json!({"op": "key", "keys": ["Return"]})).unwrap();
    match key {
        Action::Key { repeat, .. } => assert_eq!(repeat, 1),
        other => panic!("{other:?}"),
    }
}

#[test]
fn validate_mvp_table() {
    struct Case {
        patch: serde_json::Value,
        os: Os,
        class: Class,
        density: Density,
        term: Term,
        ok: bool,
        snippet: Option<&'static str>,
        err: Option<MvpError>,
    }
    let cases = [
        Case {
            patch: json!({}),
            os: Os::Linux,
            class: Class::Private,
            density: Density::Isolated,
            term: Term::OnDemand,
            ok: true,
            snippet: None,
            err: None,
        },
        Case {
            patch: json!({"density": "shared"}),
            os: Os::Linux,
            class: Class::Private,
            density: Density::Shared,
            term: Term::OnDemand,
            ok: true,
            snippet: None,
            err: None,
        },
        Case {
            patch: json!({"class": "licensed-cloud"}),
            os: Os::Linux,
            class: Class::LicensedCloud,
            density: Density::Isolated,
            term: Term::OnDemand,
            ok: false,
            snippet: Some("licensed-cloud"),
            err: Some(MvpError::UnsupportedClass(Class::LicensedCloud)),
        },
        Case {
            patch: json!({"class": "mesh"}),
            os: Os::Linux,
            class: Class::Mesh,
            density: Density::Isolated,
            term: Term::OnDemand,
            ok: false,
            snippet: Some("class=mesh"),
            err: Some(MvpError::UnsupportedClass(Class::Mesh)),
        },
        Case {
            patch: json!({"os": "windows"}),
            os: Os::Windows,
            class: Class::Private,
            density: Density::Isolated,
            term: Term::OnDemand,
            ok: false,
            snippet: Some("Windows"),
            err: Some(MvpError::UnsupportedOs(Os::Windows)),
        },
        Case {
            patch: json!({"os": "macos"}),
            os: Os::Macos,
            class: Class::Private,
            density: Density::Isolated,
            term: Term::OnDemand,
            ok: false,
            snippet: Some("macOS"),
            err: Some(MvpError::UnsupportedOs(Os::Macos)),
        },
        Case {
            patch: json!({"os": "windows", "class": "mesh"}),
            os: Os::Windows,
            class: Class::Mesh,
            density: Density::Isolated,
            term: Term::OnDemand,
            ok: false,
            snippet: Some("Windows"),
            err: Some(MvpError::UnsupportedOs(Os::Windows)),
        },
        Case {
            patch: json!({"os": "macos", "class": "mesh"}),
            os: Os::Macos,
            class: Class::Mesh,
            density: Density::Isolated,
            term: Term::OnDemand,
            ok: false,
            snippet: Some("macOS"),
            err: Some(MvpError::UnsupportedOs(Os::Macos)),
        },
        Case {
            patch: json!({"density": "exclusive"}),
            os: Os::Linux,
            class: Class::Private,
            density: Density::Exclusive,
            term: Term::OnDemand,
            ok: false,
            snippet: Some("density=exclusive"),
            err: Some(MvpError::UnsupportedDensity(Density::Exclusive)),
        },
        Case {
            patch: json!({"term": "monthly"}),
            os: Os::Linux,
            class: Class::Private,
            density: Density::Isolated,
            term: Term::Monthly,
            ok: false,
            snippet: Some("term=monthly"),
            err: Some(MvpError::UnsupportedTerm(Term::Monthly)),
        },
        Case {
            patch: json!({"term": "annual"}),
            os: Os::Linux,
            class: Class::Private,
            density: Density::Isolated,
            term: Term::Annual,
            ok: false,
            snippet: Some("term=annual"),
            err: Some(MvpError::UnsupportedTerm(Term::Annual)),
        },
    ];

    for case in cases {
        let req = parse_lease(case.patch.clone());
        assert_eq!(req.os, case.os, "patch {}", case.patch);
        assert_eq!(req.class, case.class, "patch {}", case.patch);
        assert_eq!(req.density, case.density, "patch {}", case.patch);
        assert_eq!(req.term, case.term, "patch {}", case.patch);
        let result = validate_mvp(&req);
        if case.ok {
            result.unwrap();
            Quote::from_request(&req).unwrap();
        } else {
            let err = result.unwrap_err();
            let msg = err.to_string();
            let snippet = case.snippet.expect("error snippet");
            assert!(
                msg.contains(snippet),
                "error {msg:?} should mention {snippet}"
            );
            assert_eq!(err, case.err.expect("expected error"));
            let quote_err = Quote::from_request(&req).unwrap_err();
            assert_eq!(quote_err, err);
        }
    }
}

#[test]
fn quote_seed_prices_from_math_md() {
    // 2 vCPU / 4 GiB / 40 GiB isolated Linux: 3600s → $0.0482 / hr (MATH.md).
    let isolated: LeaseRequest = load("lease_request_mvp.json");
    assert_eq!(isolated.density, Density::Isolated);
    let quote = Quote::from_request(&isolated).unwrap();
    assert_eq!(quote.os, Os::Linux);
    assert_eq!(quote.os_mult, 1.0);
    assert_eq!(quote.density_mult, DENSITY_MULT_ISOLATED);
    assert_eq!(quote.min_seconds, 60);
    assert_eq!(quote.currency, "gas");
    assert_eq!(quote.usd_per_gas, "0.01");
    assert_eq!(quote.term, Some(Term::OnDemand));

    let expected_usd = (P_CPU * 2.0 + P_MEM * 4.0 + P_DISK * 40.0) * DENSITY_MULT_ISOLATED;
    assert!((expected_usd - 0.000_013_4).abs() < 1e-15);
    let gps: f64 = quote.gas_per_second.parse().unwrap();
    assert!((gps - expected_usd / USD_PER_GAS).abs() < 1e-12);
    assert!((quote.usd_per_second().unwrap() - expected_usd).abs() < 1e-12);
    assert!((expected_usd * 3600.0 - 0.04824).abs() < 1e-9);

    let mut shared = isolated.clone();
    shared.density = Density::Shared;
    shared.min_seconds = 0;
    let shared_quote = Quote::from_request(&shared).unwrap();
    assert_eq!(shared_quote.density_mult, DENSITY_MULT_SHARED);
    assert_eq!(
        shared_quote.min_seconds,
        default_min_seconds(Os::Linux, Density::Shared)
    );
    let shared_usd = expected_usd * DENSITY_MULT_SHARED;
    assert!((shared_quote.usd_per_second().unwrap() - shared_usd).abs() < 1e-12);
    assert!((shared_usd * 3600.0 - 0.014472).abs() < 1e-9);
}

#[test]
fn scale_coordinates_table() {
    struct Case {
        xy: [i32; 2],
        from: (u32, u32),
        to: (u32, u32),
        want: [i32; 2],
    }
    let cases = [
        Case {
            xy: [100, 200],
            from: (640, 400),
            to: (1280, 800),
            want: [200, 400],
        },
        Case {
            xy: [640, 400],
            from: (1280, 800),
            to: (1280, 800),
            want: [640, 400],
        },
        Case {
            xy: [1, 1],
            from: (2, 2),
            to: (4, 4),
            want: [2, 2],
        },
        Case {
            xy: [100, 200],
            from: (0, 800),
            to: (1280, 800),
            want: [100, 200],
        },
        // Anisotropic: x doubles, y is identity. Swapping width/height would yield [100, 400].
        Case {
            xy: [100, 200],
            from: (200, 400),
            to: (400, 400),
            want: [200, 200],
        },
        // 3→4 is not integer; truncation gives 2, round-nearest gives 3.
        Case {
            xy: [2, 2],
            from: (3, 3),
            to: (4, 4),
            want: [3, 3],
        },
    ];
    for case in cases {
        let got = scale_coordinates(case.xy, case.from.0, case.from.1, case.to.0, case.to.1);
        assert_eq!(
            got, case.want,
            "xy={:?} from={:?} to={:?}",
            case.xy, case.from, case.to
        );
    }

    let mut batch: ActionBatch = load("action_batch.json");
    batch.scale_coordinates(640, 400, 1280, 800);
    match &batch.items[1] {
        Action::Click { xy, .. } => assert_eq!(*xy, [200, 400]),
        other => panic!("{other:?}"),
    }
    match &batch.items[2] {
        Action::DoubleClick { xy, .. } => assert_eq!(*xy, [200, 400]),
        other => panic!("{other:?}"),
    }
    match &batch.items[3] {
        Action::Move { xy } => assert_eq!(*xy, [200, 400]),
        other => panic!("{other:?}"),
    }
    match &batch.items[4] {
        Action::Drag { path } => assert_eq!(path.as_slice(), &[[200, 400], [600, 400]]),
        other => panic!("{other:?}"),
    }
    match &batch.items[5] {
        Action::Scroll { xy, dx, dy } => {
            assert_eq!(*xy, [200, 400]);
            assert_eq!((*dx, *dy), (0, 3));
        }
        other => panic!("{other:?}"),
    }
    match &batch.items[10] {
        Action::Zoom { region } => assert_eq!(*region, [0, 0, 400, 400]),
        other => panic!("{other:?}"),
    }
    match &batch.items[6] {
        Action::Type { text } => assert_eq!(text, "hello"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn default_min_seconds_table() {
    let cases = [
        (Os::Linux, Density::Shared, 60),
        (Os::Linux, Density::Isolated, 300),
        (Os::Linux, Density::Exclusive, 300),
        (Os::Windows, Density::Isolated, 60),
        (Os::Macos, Density::Exclusive, 86_400),
    ];
    for (os, density, want) in cases {
        assert_eq!(default_min_seconds(os, density), want);
    }
}
