#!/usr/bin/env bash
set -Eeuo pipefail

if [[ $# -ne 1 || "$1" != ghcr.io/andrescastiglia/agora@sha256:* ]]; then
  echo "usage: deploy-oracle.sh ghcr.io/andrescastiglia/agora@sha256:<digest>" >&2
  exit 2
fi

readonly image="$1"
readonly deploy_dir="${ORACLE_DEPLOY_PATH:-/opt/agora}"
readonly compose_file="$deploy_dir/compose.production.yml"
readonly current_file="$deploy_dir/.deployed-image"
readonly lock_file="$deploy_dir/.deploy.lock"

mkdir -p "$deploy_dir"
exec 9>"$lock_file"
flock -n 9 || {
  echo "another Agora deployment is running" >&2
  exit 1
}

test -r /etc/agora/agora.env
test -f "$compose_file"

previous=""
if [[ -f "$current_file" ]]; then
  previous="$(<"$current_file")"
fi

rollback() {
  if [[ -n "$previous" ]]; then
    echo "readiness failed; rolling back to the previous immutable image"
    AGORA_IMAGE="$previous" docker compose -f "$compose_file" up -d --remove-orphans
    printf '%s\n' "$previous" >"$current_file"
  fi
}
trap rollback ERR

echo "pulling immutable Agora image"
docker pull "$image"
AGORA_IMAGE="$image" docker compose -f "$compose_file" up -d --remove-orphans

for attempt in $(seq 1 30); do
  if curl --fail --silent --max-time 3 http://127.0.0.1:8088/ready >/dev/null; then
    printf '%s\n' "$image" >"$current_file"
    trap - ERR
    echo "Agora deployment is ready"
    exit 0
  fi
  sleep 2
done

echo "Agora did not become ready within 60 seconds" >&2
exit 1
