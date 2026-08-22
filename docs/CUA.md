# Cua Driver: evaluated, not adopted (2026-08-22)

`docs/MVP.md` §9 lists "Cua Driver in the image" as the first post-v0.1.0 item,
and `docs/THESIS.md` says to wrap their driver rather than race it. This is what
happened when we tried, so that revisiting it is cheap rather than a repeat.

**Outcome: not integrated.** On berth's guest it degrades to the same input
route xdotool already uses, while costing an extra ~110 MB, a supervised
daemon, and two new egress paths. The seam it would slot into now exists, so
this is a decision to revisit, not a door closed.

## What was tested

`cua-driver-rs` v0.21.0 (2026-08-19, the newest non-nightly), linux-arm64,
unpacked into the real guest image and driven against a live berth guest
(Xvfb :99, openbox, xterm, Chromium).

## What we found

**Input falls back to the route we already have.** `type_text` against the
xterm window returned:

```json
{ "code": "background_unavailable",
  "detail": "the requested target has no focus-free input backend; the
             remaining XTest/X11 route can only deliver to the globally
             focused widget" }
```

Retrying with `delivery_mode: "foreground"` works, and reports
`"route": "global_input"` with `"effect": "unverifiable"` — global XTEST
delivery to whatever holds focus. That is exactly what `xdotool` does today,
and xdotool does it without a daemon.

**The accessibility tree never came up.** The daemon logs at startup:

```
could not activate the persistent AT-SPI listener:
AT-SPI connect failed: ZBus Error: InputOutput(... NotFound ...)
```

`get_accessibility_tree` still enumerates processes and X11 windows with
bounds, but the AT-SPI element tree — semantic targeting, `element_index`,
`get_window_state` — is the thing that would justify the integration, and it is
unavailable. The guest runs `dbus-daemon` but no accessibility bus.

**This is a configuration gap, not a verdict on Cua.** A guest that launched
`at-spi-bus-launcher` and ran toolkit-accessible apps would likely get the real
behaviour. We did not pursue that: the benefit is still speculative for a
Chromium-and-xterm guest, and Chromium's a11y tree needs its own flags.

## Shape mismatches, if someone does pursue it

- **`call` needs a daemon.** `cua-driver call` talks to `cua-driver serve` over
  a socket; it is not a one-shot CLI. The entrypoint would have to start and
  supervise it, and `docker exec driver <verb>` becomes a JSON-per-action
  adapter.
- **Targeting differs.** `click` takes `(window_id + x/y)` or
  `(pid + element_index)`. berth's protocol is root-window screenshot pixels,
  so every action needs a window resolved first. `get_desktop_state` does
  return a native-size PNG and there is a desktop scope for cursor and capture.
- **`zoom` returns JPEG**; `Frame` validates PNG magic. `get_desktop_state`
  returns PNG, so the screenshot path is fine.
- **Two egress paths to close.** Telemetry is on by default (PostHog); disable
  with `CUA_DRIVER_RS_TELEMETRY_ENABLED=false`. `check-update` calls GitHub.
  Both matter because the guest is default-deny by name and address, so either
  gets blocked — correctly — and would need allowlisting to work at all.
- **Weight.** The binary is 44 MB and the layer adds ~110 MB to a 1.71 GB
  image. Runtime deps are cheap: `libX11`, `libXi`, `libxkbcommon`.
- **Maturity.** linux-arm64 binaries are recent and shipped under a
  "Pre-release" tag; the project's own `linux-support-completion-plan.md` flags
  Xvfb support as early and Unicode/drag behaviour as unproven.

Licence is MIT, compatible with ours. Avoid the optional OCR/vision extras,
which pull AGPL-3.0 `ultralytics`.

## What we did instead

Implemented the five protocol actions the guest driver was refusing — `drag`,
`hold_key`, `zoom`, `cursor_position`, plus click modifiers and non-left
double-click — in xdotool. That closed the actual, present-day gap: Anthropic's
computer-use toolset issues exactly those verbs and was getting errors.

`ACTION_BIN` now points at `/usr/local/bin/driver`, the symlink `MVP.md` always
described as the swap point but which nothing used. Repointing that symlink is
now sufficient to change drivers, which is the precondition this evaluation was
really about.

## When to revisit

When berth has a guest with a working accessibility bus, or a macOS/Windows
guest where Cua's cross-platform driver is the only realistic option. At that
point the seam is in place and the mismatches above are the work.
