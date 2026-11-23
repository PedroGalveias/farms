#!/usr/bin/env bash
# Delete every user session from Valkey/Redis, logging everyone out.
#
# Only session keys (`<prefix>:*`, default `sess:*`) are touched. Rate-limit
# counters and idempotency records live in the same database and are left
# alone; never use FLUSHALL/FLUSHDB for this (losing idempotency records lets
# a retried POST be processed twice).
#
# Runs as a dry run (count + sample) unless --yes is passed.
#
# Environment:
#   REDIS_URL           Connection URL, e.g. redis://127.0.0.1:6379 or
#                       rediss://user:pass@host:6380 (TLS is enabled for
#                       rediss://). Falls back to APP_REDIS__URI.
#   SESSION_KEY_PREFIX  `redis.session_key_prefix` from configuration/ (default: sess).
#   VALKEY_CLI          CLI to use. Default: valkey-cli, else redis-cli.
#                       For the local docker container:
#                       VALKEY_CLI="docker exec -i <container> valkey-cli"
#
# Examples:
#   REDIS_URL=redis://127.0.0.1:6379 ./scripts/clear_sessions.sh
#   REDIS_URL=redis://127.0.0.1:6379 ./scripts/clear_sessions.sh --yes
#
# See docs/clearing-sessions.md for when to use this vs. rotating the session
# secret.
set -euo pipefail

confirm=false
case "${1:-}" in
  --yes) confirm=true ;;
  "") ;;
  *)
    echo >&2 "Usage: $0 [--yes]"
    exit 2
    ;;
esac

url="${REDIS_URL:-${APP_REDIS__URI:-}}"
if [[ -z "${url}" ]]; then
  echo >&2 "Error: set REDIS_URL (or APP_REDIS__URI) to the Valkey/Redis URL."
  exit 1
fi

prefix="${SESSION_KEY_PREFIX:-sess}"
if [[ -z "${prefix}" || "${prefix}" == *"*"* ]]; then
  echo >&2 "Error: SESSION_KEY_PREFIX must be a non-empty literal prefix."
  exit 1
fi
pattern="${prefix}:*"

if [[ -n "${VALKEY_CLI:-}" ]]; then
  # Intentionally word-split, so "docker exec -i <container> valkey-cli" works.
  read -r -a cli <<<"${VALKEY_CLI}"
elif command -v valkey-cli >/dev/null 2>&1; then
  cli=(valkey-cli)
elif command -v redis-cli >/dev/null 2>&1; then
  cli=(redis-cli)
else
  echo >&2 "Error: neither valkey-cli nor redis-cli is installed (or set VALKEY_CLI)."
  exit 1
fi

cli+=(-u "${url}")
if [[ "${url}" == rediss://* ]]; then
  cli+=(--tls)
fi

# SCAN, not KEYS: KEYS blocks the server while it walks the whole keyspace.
scan_sessions() {
  "${cli[@]}" --scan --pattern "${pattern}" --count 1000
}

count="$(scan_sessions | wc -l | tr -d '[:space:]')"
echo "Found ${count} session key(s) matching '${pattern}'."

if [[ "${count}" -eq 0 ]]; then
  exit 0
fi

if [[ "${confirm}" != true ]]; then
  echo "Sample:"
  scan_sessions | head -n 5 | sed 's/^/  /'
  echo "Dry run: nothing deleted. Re-run with --yes to delete them."
  exit 0
fi

# UNLINK frees memory in the background. Session keys are alphanumeric, so
# passing them through xargs is safe. Each batch prints how many it removed.
deleted="$(
  scan_sessions |
    xargs -n 500 "${cli[@]}" UNLINK |
    awk '{ total += $1 } END { print total + 0 }'
)"
echo "Deleted ${deleted} session key(s). All users are logged out."
