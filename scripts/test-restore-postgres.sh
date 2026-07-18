#!/usr/bin/env bash
set -Eeuo pipefail

if [[ $# -ne 1 ]]; then
  echo "usage: test-restore-postgres.sh /path/to/agora-*.dump.enc" >&2
  exit 2
fi

readonly backup="$1"
readonly passphrase_file="${AGORA_BACKUP_PASSPHRASE_FILE:-/etc/agora/backup-passphrase}"
readonly restore_db="agora_restore_test"
readonly temporary="$(mktemp)"

cleanup() {
  rm -f "$temporary"
  sudo -u postgres dropdb --if-exists "$restore_db" >/dev/null
}
trap cleanup EXIT

test -r "$backup"
test -r "$passphrase_file"
openssl enc -d -aes-256-cbc -pbkdf2 -pass "file:$passphrase_file" \
  -in "$backup" -out "$temporary"
chown postgres:postgres "$temporary"
chmod 0600 "$temporary"

sudo -u postgres dropdb --if-exists "$restore_db"
sudo -u postgres createdb "$restore_db"
sudo -u postgres pg_restore --dbname "$restore_db" --no-owner --no-acl "$temporary"
sudo -u postgres psql --dbname "$restore_db" --no-psqlrc --tuples-only \
  --command "SELECT count(*) FROM webhook_events" >/dev/null

echo "Agora backup restored successfully into an isolated temporary database"
