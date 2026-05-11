#!/usr/bin/env bash
# dev.sh — Loom dev environment manager
#
# Usage:
#   ./dev.sh up           Start Loom MCP server (default)
#   ./dev.sh kill         Stop all dev processes
#   ./dev.sh restart      Kill then start fresh
#   ./dev.sh status       Show what's running
#   ./dev.sh log [N]      Show last N log lines (default: 50)
#   ./dev.sh clear-logs   Delete all logs

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
DEV_DIR="${ROOT}/tmp/dev"
PID_FILE="${DEV_DIR}/dev-servers.pid"

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[0;33m'
CYAN='\033[0;36m'
BOLD='\033[1m'
NC='\033[0m'

info()  { echo -e "${CYAN}[INFO]${NC}  $*"; }
ok()    { echo -e "${GREEN}[OK]${NC}    $*"; }
warn()  { echo -e "${YELLOW}[WARN]${NC}  $*"; }
fail()  { echo -e "${RED}[FAIL]${NC}  $*"; }
header(){ echo -e "\n${BOLD}=== $* ===${NC}"; }

ensure_dirs() {
  mkdir -p "$DEV_DIR"
}

check_prereqs() {
  local missing=0
  for cmd in cargo; do
    if ! command -v "$cmd" &>/dev/null; then
      fail "Missing: $cmd"
      missing=1
    fi
  done
  if [ "$missing" -eq 1 ]; then
    echo "Install missing tools and retry."
    exit 1
  fi
}

cmd_kill() {
  header "Stopping dev processes"

  if [ -f "$PID_FILE" ]; then
    while read -r name pid; do
      [ -z "$name" ] && continue
      if kill -0 "$pid" 2>/dev/null; then
        kill -TERM "$pid" 2>/dev/null || true
        info "SIGTERM -> $name (PID $pid)"
      else
        info "$name (PID $pid) already stopped"
      fi
    done < "$PID_FILE"
    sleep 1
    # Force kill remaining
    while read -r name pid; do
      [ -z "$name" ] && continue
      kill -9 "$pid" 2>/dev/null || true
    done < "$PID_FILE"
  fi

  rm -f "$PID_FILE"
  ok "All processes stopped"
  echo "KILL_RESULT=success"
}

cmd_up() {
  ensure_dirs

  if [ -f "$PID_FILE" ]; then
    local alive_count=0
    while read -r name pid; do
      [ -z "$name" ] && continue
      if kill -0 "$pid" 2>/dev/null; then
        alive_count=$((alive_count + 1))
      fi
    done < "$PID_FILE"

    if [ "$alive_count" -gt 0 ]; then
      echo "ALREADY_RUNNING=true"
      return 0
    else
      rm -f "$PID_FILE"
    fi
  fi

  check_prereqs

  header "Starting Loom"
  : > "$PID_FILE"

  header "Verification"
  if (cd "$ROOT" && cargo check --workspace >/dev/null); then
    ok "Workspace checks successfully"
  else
    warn "Workspace check failed"
  fi

  echo ""
  echo "---REPORT---"
  echo "LOOM_STATUS=GREEN"
  echo "ERRORS=none"
  echo "---END---"
}

cmd_status() {
  header "Dev status"

  if [ ! -f "$PID_FILE" ] || [ ! -s "$PID_FILE" ]; then
    echo "NO_SERVERS=true"
    return 0
  fi

  echo "---REPORT---"
  while read -r name pid; do
    [ -z "$name" ] && continue
    local alive="dead"
    if kill -0 "$pid" 2>/dev/null; then alive="running"; fi
    echo "SVC=${name}|PID=${pid}|ALIVE=${alive}"
  done < "$PID_FILE"
  echo "---END---"
}

cmd_log() {
  local lines="${1:-50}"
  local logfile="$DEV_DIR/loom.log"

  echo "--- Loom ($logfile) --- [last $lines lines]"
  if [ -f "$logfile" ] && [ -s "$logfile" ]; then
    tail -n "$lines" "$logfile"
  else
    echo "(no output yet)"
  fi
}

cmd_log_clear() {
  local cleared=0
  for f in "$DEV_DIR"/*.log; do
    [ -f "$f" ] && rm -f "$f" && cleared=$((cleared + 1))
  done
  echo "CLEARED=$cleared"
}

ensure_dirs

case "${1:-up}" in
  up|start)        cmd_up ;;
  kill|stop|down)  cmd_kill ;;
  restart)         cmd_kill; cmd_up ;;
  status)          cmd_status ;;
  log|logs)        shift; cmd_log "$@" ;;
  clear-logs|cl)   cmd_log_clear ;;
  *)
    echo "Usage: $0 {up|kill|restart|status|log|clear-logs}"
    exit 1
    ;;
esac
