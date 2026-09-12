#!/usr/bin/env bash
set -Eeuo pipefail
[[ $EUID == 0 ]] || { echo 'run as root' >&2; exit 1; }
readonly source_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
install -d -m 0700 /opt/agora-ops /var/lib/agora-migration
for file in runtime-common.sh backup-postgres.sh test-restore-postgres.sh migrate-oracle-k8s.sh backup-k3s.sh retire-legacy-postgres.sh observe-k8s.py database-fingerprint.sql; do
  if [[ "$source_dir/$file" != "/opt/agora-ops/$file" ]]; then install -m 0750 -o root -g root "$source_dir/$file" "/opt/agora-ops/$file"; fi
done
install -m 0644 -o root -g root "$source_dir/runtime-common.sh" /usr/local/sbin/runtime-common.sh
install -m 0750 -o root -g root "$source_dir/backup-postgres.sh" /usr/local/sbin/agora-backup-postgres
install -m 0750 -o root -g root "$source_dir/test-restore-postgres.sh" /usr/local/sbin/agora-test-restore-postgres
cat >/etc/systemd/system/agora-k3s-backup.service <<'UNIT'
[Unit]
Description=Encrypted local K3s datastore and token backup
[Service]
Type=oneshot
ExecStart=/opt/agora-ops/backup-k3s.sh
UMask=0077
Nice=10
UNIT
cat >/etc/systemd/system/agora-k3s-backup.timer <<'UNIT'
[Unit]
Description=Daily local K3s backup
[Timer]
OnCalendar=*-*-* 03:50:00
Persistent=true
[Install]
WantedBy=timers.target
UNIT
cat >/etc/systemd/system/agora-k8s-observation.service <<'UNIT'
[Unit]
Description=Agora Kubernetes operational observation
ConditionPathExists=/var/lib/agora-migration/cutover.completed
[Service]
Type=oneshot
ExecStart=/opt/agora-ops/observe-k8s.py
UMask=0077
UNIT
cat >/etc/systemd/system/agora-k8s-observation.timer <<'UNIT'
[Unit]
Description=Observe Agora every five minutes
[Timer]
OnBootSec=2min
OnUnitActiveSec=5min
[Install]
WantedBy=timers.target
UNIT
cat >/etc/systemd/system/agora-retire-legacy.service <<'UNIT'
[Unit]
Description=Retire frozen Agora source after seven days and verified recovery
ConditionPathExists=/var/lib/agora-migration/cutover.completed
[Service]
Type=oneshot
ExecStart=/opt/agora-ops/retire-legacy-postgres.sh
UMask=0077
UNIT
cat >/etc/systemd/system/agora-retire-legacy.timer <<'UNIT'
[Unit]
Description=Check whether Agora legacy PostgreSQL can be retired
[Timer]
OnCalendar=*-*-* 04:40:00
Persistent=true
[Install]
WantedBy=timers.target
UNIT
systemctl daemon-reload
# Enable only after the cutover checkpoint exists; service starts are explicit.
if [[ -f /var/lib/agora-migration/cutover.completed ]]; then
  systemctl enable --now agora-k3s-backup.timer agora-k8s-observation.timer agora-retire-legacy.timer
fi
