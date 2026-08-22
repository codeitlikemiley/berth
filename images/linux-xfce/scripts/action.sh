#!/bin/bash
# Guest-side driver shim (xdotool + ImageMagick). Swap for Cua later.
set -euo pipefail

# Always the guest Xvfb. Ignore inbound DISPLAY (docker exec Config.Env).
export DISPLAY=:99

SCROLL_MAX=40

usage() {
  cat <<'EOF' >&2
Usage: action.sh <command> [args]
  screenshot              write a PNG of DISPLAY to stdout
  zoom X Y X2 Y2          write a PNG of that region to stdout
  cursor_position         write a PNG to stdout; pointer goes to stderr
  click X Y [BUTTON] [MOD...]
                          BUTTON: left|right|middle|double (default left)
                          MOD: ctrl|alt|shift|super, held for the click
  drag X1 Y1 X2 Y2 [X Y...]
                          press at the first point, move through the rest, release
  hold_key MS KEY [KEY...]
                          hold a chord down for MS milliseconds
  type TEXT               type TEXT (non-empty stdin if TEXT is omitted)
  key KEY [KEY...]        key chord (key ctrl s  |  key Return)
  scroll X Y DY           vertical ticks at X,Y (positive is down)
  scroll X Y DX DY        both axes
  move X Y                move the pointer
  wait MS                 sleep MS milliseconds
EOF
}

map_key() {
  local k="${1// /}"
  case "$k" in
    META|Meta|meta|CMD|Cmd|cmd|SUPER|Super|super|WIN|Win|win|WINDOWS|Windows)
      echo super
      ;;
    CTRL|Ctrl|ctrl|CONTROL|Control|control)
      echo ctrl
      ;;
    ALT|Alt|alt)
      echo alt
      ;;
    SHIFT|Shift|shift)
      echo shift
      ;;
    ENTER|Enter|enter|RETURN|Return|return)
      echo Return
      ;;
    ESC|Esc|esc|ESCAPE|Escape|escape)
      echo Escape
      ;;
    SPACE|Space|space)
      echo space
      ;;
    TAB|Tab|tab)
      echo Tab
      ;;
    BACKSPACE|Backspace|backspace)
      echo BackSpace
      ;;
    DELETE|Delete|delete|DEL|Del)
      echo Delete
      ;;
    ARROW_UP|ArrowUp|ARROWUP|Up)
      echo Up
      ;;
    ARROW_DOWN|ArrowDown|ARROWDOWN|Down)
      echo Down
      ;;
    ARROW_LEFT|ArrowLeft|ARROWLEFT|Left)
      echo Left
      ;;
    ARROW_RIGHT|ArrowRight|ARROWRIGHT|Right)
      echo Right
      ;;
    PAGE_UP|PageUp|PAGEUP|Page_Up)
      echo Prior
      ;;
    PAGE_DOWN|PageDown|PAGEDOWN|Page_Down)
      echo Next
      ;;
    HOME|Home)
      echo Home
      ;;
    END|End)
      echo End
      ;;
    *)
      echo "$k"
      ;;
  esac
}

is_int() {
  [[ "${1:-}" =~ ^-?[0-9]+$ ]]
}

clamp_ticks() {
  local n="$1"
  if [ "$n" -gt "$SCROLL_MAX" ]; then
    echo "$SCROLL_MAX"
  elif [ "$n" -lt "$((-SCROLL_MAX))" ]; then
    echo "$((-SCROLL_MAX))"
  else
    echo "$n"
  fi
}

require_display() {
  if ! xdpyinfo >/dev/null 2>&1; then
    echo "action.sh: DISPLAY=${DISPLAY} is not available" >&2
    exit 1
  fi
}

emit_cursor() {
  # Frame.cursor has been in the protocol from the start and nothing ever set
  # it. stdout is the PNG, so the pointer goes on stderr behind a marker the
  # node can recognise and a human reading logs can ignore.
  local loc x y
  loc="$(xdotool getmouselocation --shell 2>/dev/null || true)"
  x="$(printf '%s\n' "$loc" | sed -n 's/^X=\([0-9-]*\)$/\1/p')"
  y="$(printf '%s\n' "$loc" | sed -n 's/^Y=\([0-9-]*\)$/\1/p')"
  if [ -n "$x" ] && [ -n "$y" ]; then
    echo "berth-cursor $x $y" >&2
  fi
}

cmd_zoom() {
  local x="${1:-}" y="${2:-}" x2="${3:-}" y2="${4:-}" w h
  if ! is_int "$x" || ! is_int "$y" || ! is_int "$x2" || ! is_int "$y2"; then
    echo "action.sh: zoom requires X Y X2 Y2" >&2
    exit 1
  fi
  # Region is [x, y, x2, y2] -- top-left and bottom-right, not width/height.
  w=$((x2 - x))
  h=$((y2 - y))
  if [ "$w" -le 0 ] || [ "$h" -le 0 ]; then
    echo "action.sh: zoom region must have positive width and height" >&2
    exit 1
  fi
  _berth_shot="$(mktemp /tmp/berth-shot.XXXXXX.png)"
  trap 'rm -f -- "${_berth_shot:-}"' EXIT
  trap 'rm -f -- "${_berth_shot:-}"; exit 141' PIPE
  if ! import -display "$DISPLAY" -silent -window root \
      -crop "${w}x${h}+${x}+${y}" +repage "png:${_berth_shot}"; then
    echo "action.sh: zoom failed" >&2
    exit 1
  fi
  emit_cursor
  cat "$_berth_shot"
}

cmd_cursor_position() {
  # Answered with a frame because the protocol has no other carrier for a
  # reply; the pointer itself rides on stderr like every other frame's does.
  cmd_screenshot
}

cmd_drag() {
  if [ "$#" -lt 4 ] || [ $(( $# % 2 )) -ne 0 ]; then
    echo "action.sh: drag requires at least two X Y pairs" >&2
    exit 1
  fi
  local -a pts=("$@") i
  for i in "${pts[@]}"; do
    if ! is_int "$i"; then
      echo "action.sh: drag coordinates must be integers" >&2
      exit 1
    fi
  done
  xdotool mousemove --sync "${pts[0]}" "${pts[1]}"
  xdotool mousedown --clearmodifiers 1
  # mouseup runs even if a move fails, so a failed drag cannot leave the guest
  # with the button stuck down.
  trap 'xdotool mouseup 1 >/dev/null 2>&1 || true' EXIT
  local n=${#pts[@]} j
  for (( j=2; j<n; j+=2 )); do
    xdotool mousemove --sync "${pts[j]}" "${pts[j+1]}"
  done
  xdotool mouseup --clearmodifiers 1
  trap - EXIT
}

cmd_hold_key() {
  local ms="${1:-}"
  shift || true
  if ! [[ "${ms}" =~ ^[0-9]+$ ]] || [ "$#" -eq 0 ]; then
    echo "action.sh: hold_key requires MS KEY [KEY...]" >&2
    exit 1
  fi
  local mapped=() k
  for k in "$@"; do
    [ -z "$k" ] && continue
    mapped+=("$(map_key "$k")")
  done
  if [ "${#mapped[@]}" -eq 0 ]; then
    echo "action.sh: hold_key requires KEY" >&2
    exit 1
  fi
  local chord
  chord="$(IFS=+; echo "${mapped[*]}")"
  xdotool keydown --clearmodifiers -- "$chord"
  # Same reasoning as drag: release even on failure rather than leaving the
  # guest with a key held down for the rest of the session.
  trap 'xdotool keyup "'"$chord"'" >/dev/null 2>&1 || true' EXIT
  python3 -c 'import time,sys; time.sleep(int(sys.argv[1])/1000.0)' "$ms"
  xdotool keyup --clearmodifiers -- "$chord"
  trap - EXIT
}

cmd_screenshot() {
  # Global so EXIT/PIPE traps can still see the path after this function returns.
  _berth_shot="$(mktemp /tmp/berth-shot.XXXXXX.png)"
  trap 'rm -f -- "${_berth_shot:-}"' EXIT
  trap 'rm -f -- "${_berth_shot:-}"; exit 141' PIPE
  if ! import -display "$DISPLAY" -silent -window root "png:${_berth_shot}"; then
    echo "action.sh: screenshot failed" >&2
    exit 1
  fi
  emit_cursor
  cat "$_berth_shot"
}

cmd_click() {
  local x="${1:-}" y="${2:-}" button="${3:-left}" b repeat=1
  if ! is_int "$x" || ! is_int "$y"; then
    echo "action.sh: click requires X Y" >&2
    exit 1
  fi
  case "$button" in
    left|1) b=1 ;;
    middle|2) b=2 ;;
    right|3) b=3 ;;
    double) b=1; repeat=2 ;;
    double_left) b=1; repeat=2 ;;
    double_middle) b=2; repeat=2 ;;
    double_right) b=3; repeat=2 ;;
    *)
      echo "action.sh: unknown button '$button'" >&2
      exit 1
      ;;
  esac
  shift 3 2>/dev/null || shift $#
  local mods=() k
  for k in "$@"; do
    [ -z "$k" ] && continue
    mods+=("$(map_key "$k")")
  done
  local chord=""
  if [ "${#mods[@]}" -gt 0 ]; then
    chord="$(IFS=+; echo "${mods[*]}")"
    xdotool keydown --clearmodifiers -- "$chord"
    trap 'xdotool keyup "'"$chord"'" >/dev/null 2>&1 || true' EXIT
  fi
  xdotool mousemove --sync "$x" "$y"
  # --clearmodifiers would undo the very modifiers we are holding.
  if [ -n "$chord" ]; then
    xdotool click --repeat "$repeat" --delay 50 "$b"
    xdotool keyup --clearmodifiers -- "$chord"
    trap - EXIT
  else
    xdotool click --clearmodifiers --repeat "$repeat" --delay 50 "$b"
  fi
}

cmd_type() {
  local text
  if [ "$#" -eq 0 ]; then
    if [ -t 0 ]; then
      echo "action.sh: type requires TEXT" >&2
      exit 1
    fi
    text="$(cat)"
    if [ -z "$text" ]; then
      echo "action.sh: type requires TEXT" >&2
      exit 1
    fi
  else
    text="$*"
  fi
  if [ -z "$text" ]; then
    return 0
  fi
  local chunk
  while [ -n "$text" ]; do
    chunk="${text:0:64}"
    text="${text:64}"
    xdotool type --clearmodifiers --delay 12 -- "$chunk"
  done
}

cmd_key() {
  if [ "$#" -eq 0 ]; then
    echo "action.sh: key requires KEY" >&2
    exit 1
  fi
  local joined parts mapped k
  joined="$(printf '%s+' "$@")"
  joined="${joined%+}"
  IFS='+' read -ra parts <<<"$joined"
  mapped=()
  for k in "${parts[@]}"; do
    [ -z "$k" ] && continue
    mapped+=("$(map_key "$k")")
  done
  if [ "${#mapped[@]}" -eq 0 ]; then
    echo "action.sh: key requires KEY" >&2
    exit 1
  fi
  local chord
  chord="$(IFS=+; echo "${mapped[*]}")"
  xdotool key --clearmodifiers -- "$chord"
}

cmd_scroll() {
  local x="${1:-}" y="${2:-}" dx dy
  if ! is_int "$x" || ! is_int "$y"; then
    echo "action.sh: scroll requires X Y DY or X Y DX DY" >&2
    exit 1
  fi
  if [ "$#" -eq 3 ]; then
    dx=0
    dy="$3"
  elif [ "$#" -eq 4 ]; then
    dx="$3"
    dy="$4"
  else
    echo "action.sh: scroll requires X Y DY or X Y DX DY" >&2
    exit 1
  fi
  if ! is_int "$dx" || ! is_int "$dy"; then
    echo "action.sh: scroll ticks must be integers" >&2
    exit 1
  fi
  dx="$(clamp_ticks "$dx")"
  dy="$(clamp_ticks "$dy")"
  xdotool mousemove --sync "$x" "$y"
  if [ "$dy" -gt 0 ]; then
    xdotool click --clearmodifiers --repeat "$dy" --delay 20 5
  elif [ "$dy" -lt 0 ]; then
    xdotool click --clearmodifiers --repeat "$((-dy))" --delay 20 4
  fi
  if [ "$dx" -gt 0 ]; then
    xdotool click --clearmodifiers --repeat "$dx" --delay 20 7
  elif [ "$dx" -lt 0 ]; then
    xdotool click --clearmodifiers --repeat "$((-dx))" --delay 20 6
  fi
}

cmd_move() {
  local x="${1:-}" y="${2:-}"
  if ! is_int "$x" || ! is_int "$y"; then
    echo "action.sh: move requires X Y" >&2
    exit 1
  fi
  xdotool mousemove --sync "$x" "$y"
}

cmd_wait() {
  local ms="${1:-}"
  if ! [[ "${ms}" =~ ^[0-9]+$ ]]; then
    echo "action.sh: wait requires MS milliseconds" >&2
    exit 1
  fi
  python3 -c 'import time,sys; time.sleep(int(sys.argv[1])/1000.0)' "$ms"
}

if [ "${1:-}" = "-h" ] || [ "${1:-}" = "--help" ] || [ $# -lt 1 ]; then
  usage
  if [ $# -lt 1 ]; then
    exit 2
  fi
  exit 0
fi

op="$1"
shift

case "$op" in
  screenshot|zoom|cursor_position|click|drag|type|key|hold_key|scroll|move|wait) ;;
  *)
    echo "action.sh: unknown command '$op'" >&2
    usage
    exit 2
    ;;
esac

if [ "$op" != "wait" ]; then
  require_display
fi

case "$op" in
  screenshot) cmd_screenshot ;;
  zoom) cmd_zoom "$@" ;;
  cursor_position) cmd_cursor_position ;;
  click) cmd_click "$@" ;;
  drag) cmd_drag "$@" ;;
  hold_key) cmd_hold_key "$@" ;;
  type) cmd_type "$@" ;;
  key) cmd_key "$@" ;;
  scroll) cmd_scroll "$@" ;;
  move) cmd_move "$@" ;;
  wait) cmd_wait "$@" ;;
esac
