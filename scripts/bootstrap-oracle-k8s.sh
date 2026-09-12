#!/usr/bin/env bash
# Run from a trusted release checkout on oracle, after installing pinned K3s.
set -Eeuo pipefail
[[ $EUID == 0 ]] || { echo 'run as root' >&2; exit 1; }
readonly script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
readonly manifests="$script_dir/../k8s"
[[ "$(k3s kubectl get node oracle -o jsonpath='{.status.nodeInfo.architecture}')" == arm64 ]]
install -d -o 999 -g 999 -m 0700 /var/lib/agora-k8s/postgres
k3s kubectl apply -f "$manifests/bootstrap.yaml"
# Must precede StatefulSet creation: the container requires this Secret at startup.
python3 "$script_dir/configure-k8s-secrets.py" --postgres-admin
k3s kubectl apply -k "$manifests/database"
for attempt in $(seq 1 60); do
  k3s kubectl get pod postgres-0 -n agora >/dev/null 2>&1 && break
  sleep 2
done
k3s kubectl wait -n agora --for=condition=Ready pod/postgres-0 --timeout=300s
python3 "$script_dir/configure-k8s-secrets.py" --application
bash "$script_dir/install-k8s-operations.sh"
echo 'Database and operations prepared. No production application or traffic activated.'
