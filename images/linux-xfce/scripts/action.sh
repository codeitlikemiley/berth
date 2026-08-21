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
  click X Y [BUTTON]      BUTTON: left|right|middle|double (default left)
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

cmd_screenshot() {
  # Global so EXIT/PIPE traps can still see the path after this function returns.
  _berth_shot="$(mktemp /tmp/berth-shot.XXXXXX.png)"
  trap 'rm -f -- "${_berth_shot:-}"' EXIT
  trap 'rm -f -- "${_berth_shot:-}"; exit 141' PIPE
  if ! import -display "$DISPLAY" -silent -window root "png:${_berth_shot}"; then
    echo "action.sh: screenshot failed" >&2
    exit 1
  fi
  cat "$_berth_shot"
}

cmd_click() {
  local x="${1:-}" y="${2:-}" button="${3:-left}" b
  if ! is_int "$x" || ! is_int "$y"; then
    echo "action.sh: click requires X Y" >&2
    exit 1
  fi
  case "$button" in
    left|1) b=1 ;;
    middle|2) b=2 ;;
    right|3) b=3 ;;
    double)
      xdotool mousemove --sync "$x" "$y" click --clearmodifiers --repeat 2 --delay 50 1
      return
      ;;
    *)
      echo "action.sh: unknown button '$button'" >&2
      exit 1
      ;;
  esac
  xdotool mousemove --sync "$x" "$y" click --clearmodifiers "$b"
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
  screenshot|click|type|key|scroll|move|wait) ;;
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
  click) cmd_click "$@" ;;
  type) cmd_type "$@" ;;
  key) cmd_key "$@" ;;
  scroll) cmd_scroll "$@" ;;
  move) cmd_move "$@" ;;
  wait) cmd_wait "$@" ;;
esac
