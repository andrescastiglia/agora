#!/usr/bin/env bash
set -Eeuo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/runtime-common.sh"
if [[ $# != 1 || ! "$1" =~ ^ghcr.io/andrescastiglia/agora@sha256:[0-9a-f]{64}$ ]]; then
  echo 'usage: deploy-oracle.sh ghcr.io/andrescastiglia/agora@sha256:<digest>' >&2
  exit 2
fi
readonly image="$1"
readonly deploy_dir="${ORACLE_DEPLOY_PATH:-/opt/agora}"
readonly current_file="$deploy_dir/.deployed-image"
mkdir -p "$deploy_dir"
exec 9>"$deploy_dir/.deploy.lock"
flock -n 9 || { echo 'another Agora operation is running' >&2; exit 1; }
previous=""
[[ ! -f "$current_file" ]] || previous="$(<"$current_file")"
work="$(mktemp -d)"
rollback_needed=0
kubectl_deploy() { k3s kubectl --kubeconfig /etc/agora/kubeconfig-deploy -n agora "$@"; }
deploy_image() {
  local target="$1"
  if [[ "$AGORA_RUNTIME_BACKEND" == kubernetes ]]; then
    cp -R "$deploy_dir/k8s/app/." "$work/" || return 1
    sed -i "s/digest: sha256:[0-9a-f]*/digest: ${target##*@}/" "$work/kustomization.yaml" || return 1
    kubectl_deploy apply -k "$work" || return 1
    kubectl_deploy rollout status deployment/agora --timeout=600s || return 1
    curl --fail --silent --max-time 10 http://127.0.0.1:30088/ready >/dev/null || return 1
  else
    docker pull "$target" || return 1
    AGORA_IMAGE="$target" docker compose -f "$deploy_dir/compose.oracle.yml" up -d --remove-orphans || return 1
    for attempt in $(seq 1 60); do
      if curl --fail --silent --max-time 3 http://127.0.0.1:8088/ready >/dev/null; then return 0; fi
      sleep 2
    done
    return 1
  fi
}
finish() {
  local result=$?
  trap - EXIT
  if [[ "$result" != 0 && "$rollback_needed" == 1 && -n "$previous" ]]; then
    echo 'deployment failed; restoring previous immutable application image' >&2
    if ! deploy_image "$previous"; then
      echo 'CRITICAL: application rollback failed; database was not reverted' >&2
    fi
  fi
  rm -rf -- "$work"
  exit "$result"
}
trap finish EXIT
rollback_needed=1
if ! deploy_image "$image"; then echo 'Agora deployment or readiness failed' >&2; exit 1; fi
if ! curl --fail --silent --max-time 15 https://agora.maese.com.ar/ready >/dev/null; then
  echo 'public readiness failed' >&2; exit 1
fi
printf '%s\n' "$image" >"$current_file"
rollback_needed=0
echo 'Agora deployment is ready'
