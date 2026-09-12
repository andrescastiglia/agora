#!/usr/bin/env bash
# Explicit, resumable maintenance operations. Run on oracle as root.
set -Eeuo pipefail
[[ $EUID == 0 ]] || { echo 'run as root' >&2; exit 1; }
source "$(dirname "${BASH_SOURCE[0]}")/runtime-common.sh"
readonly script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
readonly state_dir=/var/lib/agora-migration
readonly backup_dir=/var/backups/agora-migration
readonly passphrase=/etc/agora/backup-passphrase
install -d -m 0700 "$state_dir" "$backup_dir"
exec 9>/opt/agora/.deploy.lock
flock -n 9 || { echo 'another Agora operation is running' >&2; exit 1; }
pg_host() { sudo -u postgres -H "$@"; }
pg_pod() { k3s kubectl exec -i -n agora postgres-0 -- "$@" -U postgres; }
fingerprint_host() { pg_host psql -X -At -d "$1" <"$script_dir/database-fingerprint.sql"; }
fingerprint_pod() { pg_pod psql -X -At -d "$1" <"$script_dir/database-fingerprint.sql"; }
set_runtime() {
  printf 'AGORA_RUNTIME_BACKEND=%s\nAGORA_DATABASE_NAME=%s\n' "$1" "$2" > /etc/agora/runtime.conf.tmp
  chown root:deploy /etc/agora/runtime.conf.tmp
  chmod 0640 /etc/agora/runtime.conf.tmp
  mv /etc/agora/runtime.conf.tmp /etc/agora/runtime.conf
}
set_port() {
  cp /etc/nginx/conf.d/agora-upstream.conf "$state_dir/upstream.previous"
  printf 'upstream agora_backend { server 127.0.0.1:%s; }\n' "$1" >/etc/nginx/conf.d/agora-upstream.conf
  if ! nginx -t; then cp "$state_dir/upstream.previous" /etc/nginx/conf.d/agora-upstream.conf; return 1; fi
  systemctl reload nginx
}
maintenance() { touch /etc/nginx/agora-webhooks-maintenance; }
release_maintenance() { rm -f /etc/nginx/agora-webhooks-maintenance; }
case "${1:-}" in
  cutover)
    [[ "$AGORA_RUNTIME_BACKEND" == compose ]]
    # Update the actual systemd executables, not only uploaded release files.
    bash "$script_dir/install-k8s-operations.sh"
    [[ ! -e "$state_dir/cutover.started" ]] || { echo 'cutover already started; inspect state or rollback' >&2; exit 1; }
    test -f "$state_dir/preflight-ok"
    # Fresh operational preflight; its 15-minute sampling is recorded separately.
    [[ $(($(date +%s)-$(stat -c %Y "$state_dir/preflight-ok"))) -lt 3600 ]]
    [[ $(df -B1 --output=avail / | tail -1) -gt 32212254720 ]]
    test -f /etc/nginx/conf.d/agora-upstream.conf
    k3s kubectl wait -n agora --for=condition=Ready pod/postgres-0 --timeout=120s
    [[ "$(pg_pod psql -X -Atc "SELECT count(*) FROM pg_database WHERE datname='agora'")" == 0 ]]
    date -u +%FT%TZ >"$state_dir/cutover.started"
    maintenance
    systemctl stop agora-backup.timer
    # Wait for a currently running backup before freezing writers.
    while systemctl is-active --quiet agora-backup.service; do sleep 2; done
    provider="$(python3 "$script_dir/update-database-url.py" --provider /etc/agora/agora.env)"
    [[ "$provider" == telegram || "$provider" == whatsapp ]]
    drained=0
    for attempt in $(seq 1 360); do
      count="$(pg_host psql -X -At -d agora -c "SELECT (SELECT count(*) FROM jobs WHERE provider='$provider' AND status IN ('pending','processing'))+(SELECT count(*) FROM webhook_events WHERE provider='$provider' AND processing_status IN ('pending','processing'))")"
      if [[ "$count" == 0 ]]; then drained=1; break; fi
      sleep 5
    done
    [[ "$drained" == 1 ]] || { echo 'queue did not drain; maintenance remains enabled' >&2; exit 1; }
    sudo -u deploy -H docker update --restart=no agora-api >/dev/null
    sudo -u deploy -H docker stop --time 330 agora-api >/dev/null
    pg_host psql -X -v ON_ERROR_STOP=1 -d agora -c 'ALTER ROLE agora NOLOGIN;' >/dev/null
    [[ "$(pg_host psql -X -At -d agora -c "SELECT count(*) FROM pg_stat_activity WHERE datname='agora' AND usename='agora'")" == 0 ]]
    pg_host pg_dump -Fc --no-acl agora | openssl enc -aes-256-cbc -salt -pbkdf2 -pass "file:$passphrase" -out "$backup_dir/final.dump.enc"
    fingerprint_host agora >"$state_dir/source.fingerprint"
    pg_pod createdb -O agora --template=template0 --encoding=UTF8 --locale=C.UTF-8 agora
    openssl enc -d -aes-256-cbc -pbkdf2 -pass "file:$passphrase" -in "$backup_dir/final.dump.enc" | pg_pod pg_restore --single-transaction --exit-on-error -d agora
    fingerprint_pod agora >"$state_dir/target.fingerprint"
    diff -u "$state_dir/source.fingerprint" "$state_dir/target.fingerprint"
    touch "$state_dir/restore-verified"
    # Bootstrap app resources as administrator; subsequent releases use limited RBAC.
    image="$(cat /opt/agora/.deployed-image)"
    work="$(mktemp -d)"
    cp -R /opt/agora/k8s/app/. "$work/"
    sed -i "s/digest: sha256:[0-9a-f]*/digest: ${image##*@}/" "$work/kustomization.yaml"
    # From this point the worker can write, even before public traffic opens.
    touch "$state_dir/target.activated"
    k3s kubectl apply -k "$work"
    rm -rf "$work"
    k3s kubectl rollout status deployment/agora -n agora --timeout=600s
    curl -fsS --max-time 10 http://127.0.0.1:30088/ready >/dev/null
    set_port 30088
    set_runtime kubernetes agora
    backup="$(/usr/local/sbin/agora-backup-postgres)"
    /usr/local/sbin/agora-test-restore-postgres "$backup"
    printf 'agora\n' >/etc/agora/legacy-databases
    date -u +%FT%TZ >"$state_dir/cutover.completed"
    bash "$script_dir/install-k8s-operations.sh"
    release_maintenance
    systemctl start agora-backup.timer
    curl -fsS --max-time 15 https://agora.maese.com.ar/ready
    ;;
  rollback)
    maintenance
    systemctl stop agora-backup.timer
    while systemctl is-active --quiet agora-backup.service; do sleep 2; done
    if k3s kubectl get deployment agora -n agora >/dev/null 2>&1; then
      k3s kubectl scale deployment/agora -n agora --replicas=0
      k3s kubectl wait -n agora --for=delete pod -l app=agora --timeout=360s
    fi
    if [[ -e "$state_dir/target.activated" ]]; then
      # Always copy current state after activation; never use the obsolete host DB.
      rollback_db="agora_rollback_$(date -u +%Y%m%d%H%M%S)"
      pg_pod pg_dump -Fc --no-acl agora | openssl enc -aes-256-cbc -salt -pbkdf2 -pass "file:$passphrase" -out "$backup_dir/rollback.dump.enc"
      pg_host createdb -O agora --template=template0 --encoding=UTF8 --locale=C.UTF-8 "$rollback_db"
      openssl enc -d -aes-256-cbc -pbkdf2 -pass "file:$passphrase" -in "$backup_dir/rollback.dump.enc" | pg_host pg_restore --single-transaction --exit-on-error -d "$rollback_db"
      fingerprint_pod agora >"$state_dir/rollback-source.fingerprint"
      fingerprint_host "$rollback_db" >"$state_dir/rollback-target.fingerprint"
      diff -u "$state_dir/rollback-source.fingerprint" "$state_dir/rollback-target.fingerprint"
      python3 "$script_dir/update-database-url.py" /etc/agora/agora.env "$rollback_db"
    else
      rollback_db=agora
    fi
    pg_host psql -X -v ON_ERROR_STOP=1 -d "$rollback_db" -c 'ALTER ROLE agora LOGIN;' >/dev/null
    image="$(cat /opt/agora/.deployed-image)"
    sudo -u deploy -H env AGORA_IMAGE="$image" docker compose -f /opt/agora/compose.oracle.yml up -d
    for attempt in $(seq 1 60); do curl -fsS --max-time 3 http://127.0.0.1:8088/ready >/dev/null && break; sleep 2; done
    curl -fsS --max-time 3 http://127.0.0.1:8088/ready >/dev/null
    set_runtime compose "$rollback_db"
    set_port 8088
    date -u +%FT%TZ >"$state_dir/rollback.completed"
    systemctl disable --now agora-k8s-observation.timer agora-retire-legacy.timer
    release_maintenance
    systemctl start agora-backup.timer
    echo 'Compose restored using the verified current database'
    ;;
  *) echo 'usage: migrate-oracle-k8s.sh cutover|rollback' >&2; exit 2;;
esac
