#!/usr/bin/env bash
# Only retires the exact frozen Agora source after seven days and verified recovery.
set -Eeuo pipefail
[[ $EUID == 0 ]] || exit 1
source "$(dirname "${BASH_SOURCE[0]}")/runtime-common.sh"
[[ "$AGORA_RUNTIME_BACKEND" == kubernetes ]] || exit 0
[[ -f /etc/agora/legacy-databases && -s /etc/agora/legacy-databases ]] || exit 0
exec 9>/opt/agora/.deploy.lock
flock -n 9 || exit 1
cutover_epoch="$(date -d "$(cat /var/lib/agora-migration/cutover.completed)" +%s)"
if (( $(date +%s) - cutover_epoch < 604800 )); then exit 0; fi
python3 - <<'PY'
import datetime,json,time
from pathlib import Path
p=Path('/var/lib/agora-migration/cutover.completed')
start=datetime.datetime.fromisoformat(p.read_text().strip().replace('Z','+00:00')).timestamp()
now=time.time()
if now-start<7*86400:raise SystemExit(1)
rows=[json.loads(l) for l in Path('/var/log/agora-k8s-observation.jsonl').read_text().splitlines()]
rows=[r for r in rows if r['epoch']>=now-48*3600]
if not rows or rows[0]['epoch']>now-48*3600+600 or rows[-1]['epoch']<now-600:raise SystemExit(1)
if any(not r['ok'] for r in rows):raise SystemExit(1)
if any(b['epoch']-a['epoch']>600 for a,b in zip(rows,rows[1:])):raise SystemExit(1)
PY
[[ "$(cat /etc/agora/legacy-databases)" == agora ]] || exit 1
[[ "$(sudo -u postgres -H psql -X -At -d postgres -c "SELECT rolcanlogin FROM pg_roles WHERE rolname='agora'")" == f ]] || exit 1
backup="$(/usr/local/sbin/agora-backup-postgres)"
/usr/local/sbin/agora-test-restore-postgres "$backup"
sudo -u postgres -H dropdb agora
: >/etc/agora/legacy-databases
find /var/backups/agora-migration -maxdepth 1 -type f -name '*.enc' -delete
date -u +%FT%TZ >/var/lib/agora-migration/legacy.retired
