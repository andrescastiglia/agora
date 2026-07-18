#!/usr/bin/env bash
set -Eeuo pipefail

readonly backup_dir="${AGORA_BACKUP_DIR:-/var/backups/agora}"
readonly passphrase_file="${AGORA_BACKUP_PASSPHRASE_FILE:-/etc/agora/backup-passphrase}"
readonly timestamp="$(date -u +%Y%m%dT%H%M%SZ)"
readonly destination="$backup_dir/agora-$timestamp.dump.enc"
readonly temporary="$destination.tmp"

install -d -o root -g root -m 0700 "$backup_dir"
test -r "$passphrase_file"
trap 'rm -f "$temporary"' EXIT

sudo -u postgres pg_dump --format=custom --no-owner --no-acl agora |
  openssl enc -aes-256-cbc -salt -pbkdf2 -pass "file:$passphrase_file" \
    -out "$temporary"

chmod 0600 "$temporary"
mv "$temporary" "$destination"
find "$backup_dir" -type f -name 'agora-*.dump.enc' -mtime +14 -delete

echo "$destination"
