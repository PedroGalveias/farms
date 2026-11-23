# Clearing user sessions

Sessions are stored in Valkey/Redis by `actix-session`'s `RedisSessionStore`,
under keys named `<redis.session_key_prefix>:<session key>` (`sess:...` in
every environment). The browser only holds a signed cookie with the session
key; the cookie is signed with a key derived from `session.secret_key`
(`SESSION_SECRET_KEY` in production).

There are two ways to log everyone out.

## When you need this

- **Rolling back actix-session 0.12 → 0.11.** 0.12 stores session state in a
  new versioned format and migrates 0.11 sessions when it loads them, but 0.11
  can't read sessions written by 0.12. After such a rollback, clear sessions
  (users log in again) rather than letting requests fail to load them.
- **Suspected session compromise** or a leaked session secret: rotate the
  secret (option 1); clearing Valkey alone doesn't help if the secret leaked.

Upgrading 0.11 → 0.12 by itself does **not** need this.

## Option 1: rotate the session secret

Change `session.secret_key` (production: the `SESSION_SECRET_KEY` environment
variable) and restart the app. Every existing cookie fails signature
verification, so every user is logged out immediately. The old session
entries in Valkey are never read again and expire on their own TTL
(`session.ttl_seconds`).

The secret must be at least 32 bytes (`Key::derive_from` panics on shorter
input). Generate one with:

```bash
openssl rand -base64 64
```

## Option 2: delete the session keys

[`scripts/clear_sessions.sh`](../scripts/clear_sessions.sh) scans for
`<prefix>:*` and deletes only those keys with `UNLINK`. It is a dry run
(count + sample) unless `--yes` is passed.

```bash
# Dry run
REDIS_URL='rediss://user:password@host:6380' ./scripts/clear_sessions.sh

# Delete
REDIS_URL='rediss://user:password@host:6380' ./scripts/clear_sessions.sh --yes
```

- Uses `valkey-cli`, falling back to `redis-cli`. `rediss://` URLs get `--tls`.
- Against the local docker-compose / `init_redis.sh` container, without a CLI
  on the host:

  ```bash
  REDIS_URL=redis://127.0.0.1:6379 \
  VALKEY_CLI="docker exec -i <valkey-container> valkey-cli" \
    ./scripts/clear_sessions.sh --yes
  ```

  (`docker ps` shows the container name. The URL is resolved inside the
  container, so 127.0.0.1 is the container's own Valkey.)
- Set `SESSION_KEY_PREFIX` if `redis.session_key_prefix` was changed from
  `sess`.
- The password in `REDIS_URL` is visible in the process list while the CLI
  runs. Run it from a trusted machine.

Equivalent manual commands:

```bash
valkey-cli -u "$REDIS_URL" --scan --pattern 'sess:*' --count 1000 | wc -l
valkey-cli -u "$REDIS_URL" --scan --pattern 'sess:*' --count 1000 \
  | xargs -n 500 valkey-cli -u "$REDIS_URL" UNLINK
```

**Don't** use `KEYS *` on a production instance (it blocks the server while it
walks every key), and **don't** use `FLUSHALL`/`FLUSHDB`: the same database
holds rate-limit counters and idempotency records, and losing the latter lets
a retried POST be processed twice.
