#!/usr/bin/env bash
# alloc-ports.sh — Allocate unique ports for a worktree pipeline
#
# Usage:
#   ./.claude/scripts/alloc-ports.sh alloc <worktree-id>
#   ./.claude/scripts/alloc-ports.sh free  <worktree-id>
#   ./.claude/scripts/alloc-ports.sh list

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
REGISTRY="${ROOT}/.worktrees/.ports"
LOCKFILE="${REGISTRY}.lock"

# Loom is a single MCP server — one port per pipeline
MCP_BASE=8100
MAX_SLOTS=99

mkdir -p "$(dirname "$REGISTRY")"
touch "$REGISTRY"

acquire_lock() {
  local tries=0
  while ! mkdir "$LOCKFILE" 2>/dev/null; do
    tries=$((tries + 1))
    if [ "$tries" -gt 50 ]; then
      echo "Error: could not acquire port lock after 5s" >&2
      exit 1
    fi
    sleep 0.1
  done
  trap 'rmdir "$LOCKFILE" 2>/dev/null' EXIT
}

cmd_alloc() {
  local id="$1"
  acquire_lock

  local existing
  existing=$(awk -v id="$id" '$1 == id { print $0 }' "$REGISTRY")
  if [ -n "$existing" ]; then
    local mcp_port
    mcp_port=$(echo "$existing" | awk '{print $2}')
    echo "MCP_PORT=${mcp_port}"
    return 0
  fi

  port_is_free() {
    ! lsof -i ":${1}" -sTCP:LISTEN >/dev/null 2>&1
  }

  local slot=0
  while [ "$slot" -lt "$MAX_SLOTS" ]; do
    local mcp_port=$((MCP_BASE + slot))
    if ! awk -v p="${mcp_port}" '$2 == p' "$REGISTRY" | grep -q .; then
      if port_is_free "$mcp_port"; then
        echo "${id} ${mcp_port}" >> "$REGISTRY"
        echo "MCP_PORT=${mcp_port}"
        return 0
      fi
    fi
    slot=$((slot + 1))
  done

  echo "Error: no free port slots (all $MAX_SLOTS in use)" >&2
  exit 1
}

cmd_free() {
  local id="$1"
  acquire_lock
  grep -v "^${id} " "$REGISTRY" > "${REGISTRY}.tmp" 2>/dev/null || true
  mv "${REGISTRY}.tmp" "$REGISTRY"
  echo "Freed ports for: $id"
}

cmd_list() {
  if [ ! -s "$REGISTRY" ]; then
    echo "No port allocations."
    return 0
  fi
  printf "%-40s %-10s\n" "WORKTREE" "MCP_PORT"
  while IFS= read -r line; do
    local id mcp
    id=$(echo "$line" | awk '{print $1}')
    mcp=$(echo "$line" | awk '{print $2}')
    printf "%-40s %-10s\n" "$id" "$mcp"
  done < "$REGISTRY"
}

case "${1:-help}" in
  alloc) cmd_alloc "${2:?worktree-id required}" ;;
  free)  cmd_free  "${2:?worktree-id required}" ;;
  list)  cmd_list ;;
  *)
    echo "Usage: $0 {alloc|free|list} [worktree-id]"
    exit 1
    ;;
esac
