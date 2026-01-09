#!/usr/bin/env bash
set -x
set -eo pipefail

RUNNING_REDIS_CONTAINER=$(docker ps --filter 'name=valkey' --format '{{.ID}}')

if [[ -n $RUNNING_REDIS_CONTAINER ]]; then
  echo >&2 "there is already a redis container running, kill it with:"
  echo >&2 "\tdocker kill ${RUNNING_REDIS_CONTAINER}"
  exit 1
fi

# Launch new valkey using docker
docker run \
  --publish 6379:6379 \
  --detach \
  --name "valkey_$(date '+%s')" \
  valkey/valkey:9-alpine

echo >&2 "Valkey (Redis) is ready to go"
