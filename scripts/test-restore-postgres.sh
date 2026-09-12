#!/usr/bin/env bash
set -Eeuo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/runtime-common.sh"
[[ $# == 1 ]] || { echo 'usage: test-restore-postgres.sh encrypted-dump' >&2; exit 2; }
readonly backup="$1"
readonly passphrase_file="${AGORA_BACKUP_PASSPHRASE_FILE:-/etc/agora/backup-passphrase}"
readonly restore_db="agora_restore_test_$(date +%s)_$$"
created=0
pg() {
  if [[ "$AGORA_RUNTIME_BACKEND" == kubernetes ]]; then
    agora_kubectl exec -i postgres-0 -- "$@" -U postgres
  else
    sudo -u postgres -H "$@"
  fi
}
cleanup() {
  if [[ "$created" == 1 ]]; then pg dropdb --if-exists "$restore_db" >/dev/null; fi
}
trap cleanup EXIT
test -r "$backup"
test -r "$passphrase_file"
pg createdb --template=template0 --encoding=UTF8 --locale=C.UTF-8 "$restore_db"
created=1
openssl enc -d -aes-256-cbc -pbkdf2 -pass "file:$passphrase_file" -in "$backup" |
  pg pg_restore --dbname "$restore_db" --single-transaction --exit-on-error
pg psql --dbname "$restore_db" -X -v ON_ERROR_STOP=1 -At \
  -c 'SELECT count(*) FROM webhook_events; SELECT count(*) FROM _sqlx_migrations WHERE success; SELECT count(*) FROM attachments; SELECT count(*) FROM document_chunks;' >/dev/null
echo 'Agora backup restored successfully into an isolated temporary database'
