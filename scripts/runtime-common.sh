#!/usr/bin/env bash
# Source from operational scripts; backend selection never falls back silently.
runtime_file="${AGORA_RUNTIME_CONFIG:-/etc/agora/runtime.conf}"
if [[ -f "$runtime_file" ]]; then
  # The production file is root-owned and not writable by deploy.
  source "$runtime_file"
fi
: "${AGORA_RUNTIME_BACKEND:=compose}"
: "${AGORA_DATABASE_NAME:=agora}"
case "$AGORA_RUNTIME_BACKEND" in compose|kubernetes) ;; *) echo 'invalid Agora runtime backend' >&2; exit 2;; esac
agora_kubectl() { k3s kubectl -n agora "$@"; }
agora_psql() {
  if [[ "$AGORA_RUNTIME_BACKEND" == kubernetes ]]; then
    agora_kubectl exec -i postgres-0 -- psql -U postgres --dbname "$AGORA_DATABASE_NAME" "$@"
  elif [[ -n ${AGORA_PSQL_DOCKER_SERVICE:-} ]]; then
    docker compose exec -T "$AGORA_PSQL_DOCKER_SERVICE" psql -U "${AGORA_DATABASE_USER:-agora}" --dbname "$AGORA_DATABASE_NAME" "$@"
  else
    sudo -u postgres -H psql --dbname "$AGORA_DATABASE_NAME" "$@"
  fi
}
agora_dump() {
  if [[ "$AGORA_RUNTIME_BACKEND" == kubernetes ]]; then
    agora_kubectl exec postgres-0 -- pg_dump -U postgres --format=custom --no-acl "$AGORA_DATABASE_NAME"
  else
    sudo -u postgres -H pg_dump --format=custom --no-acl "$AGORA_DATABASE_NAME"
  fi
}
agora_stop() {
  if [[ "$AGORA_RUNTIME_BACKEND" == kubernetes ]]; then
    agora_kubectl scale deployment/agora --replicas=0 >/dev/null
    agora_kubectl wait --for=delete pod -l app=agora --timeout=360s >/dev/null
  else
    docker stop --time 330 "${AGORA_RUNTIME_CONTAINER:-agora-api}" >/dev/null
  fi
}
agora_start() {
  if [[ "$AGORA_RUNTIME_BACKEND" == kubernetes ]]; then
    agora_kubectl scale deployment/agora --replicas=1 >/dev/null
    agora_kubectl rollout status deployment/agora --timeout=600s >/dev/null
  else
    docker start "${AGORA_RUNTIME_CONTAINER:-agora-api}" >/dev/null
  fi
}
