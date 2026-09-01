#!/usr/bin/env bash
set -Eeuo pipefail

if [[ $# -ne 4 || $1 != "--confirm" ]]; then
  echo "usage: delete-participant-data.sh --confirm <--replace-backups|--test-no-backups> <telegram|whatsapp> <participant-id>" >&2
  exit 2
fi

readonly backup_mode="$2"
readonly provider="$3"
readonly participant_id="$4"
readonly script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
readonly database_name="${AGORA_DATABASE_NAME:-agora}"
readonly audit_log="${AGORA_DELETION_AUDIT_LOG:-/var/log/agora-data-deletions.log}"

if [[ $backup_mode != "--replace-backups" && $backup_mode != "--test-no-backups" ]]; then
  echo "backup mode must be --replace-backups or --test-no-backups" >&2
  exit 2
fi
if [[ $backup_mode == "--test-no-backups" && ${AGORA_ALLOW_TEST_NO_BACKUPS:-0} != "1" ]]; then
  echo "--test-no-backups is allowed only with AGORA_ALLOW_TEST_NO_BACKUPS=1" >&2
  exit 2
fi
if [[ $provider != "telegram" && $provider != "whatsapp" ]]; then
  echo "provider must be telegram or whatsapp" >&2
  exit 2
fi
if [[ ! $participant_id =~ ^[A-Za-z0-9_.:+-]+$ ]]; then
  echo "participant-id contains unsupported characters" >&2
  exit 2
fi

audit_dir="$(dirname "$audit_log")"
if [[ ! -d $audit_dir ]]; then
  install -d -m 0750 "$audit_dir"
fi
touch "$audit_log"
chmod 0600 "$audit_log"
test -w "$audit_log"

if [[ $backup_mode == "--replace-backups" ]]; then
  readonly backup_dir="${AGORA_BACKUP_DIR:-/var/backups/agora}"
  readonly backup_command="${AGORA_BACKUP_COMMAND:-/usr/local/sbin/agora-backup-postgres}"
  test -d "$backup_dir"
  test -x "$backup_command"
fi

if [[ -n ${AGORA_PSQL_DOCKER_SERVICE:-} ]]; then
  psql_command=(
    docker compose exec -T "$AGORA_PSQL_DOCKER_SERVICE"
    psql --username "${AGORA_DATABASE_USER:-agora}" --dbname "$database_name"
  )
else
  psql_command=(sudo -u postgres -H psql --dbname "$database_name")
fi

summary="$(
  "${psql_command[@]}" --no-psqlrc --set ON_ERROR_STOP=1 --quiet --tuples-only --no-align \
    < <(
      printf "SET agora.provider = '%s'; SET agora.participant_id = '%s';\n" \
        "$provider" "$participant_id"
      sed -n '1,$p' "$script_dir/delete-participant-data.sql"
    )
)"
test -n "$summary"

write_audit() {
  local backup_status="$1"
  printf '%s backup_replacement=%s %s\n' \
    "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$backup_status" "$summary" >>"$audit_log"
}

if [[ $backup_mode == "--replace-backups" ]]; then
  write_audit "pending"
  if ! new_backup="$($backup_command)"; then
    write_audit "failed"
    echo "replacement backup failed; rerun the same command to resume cleanup" >&2
    exit 1
  fi
  new_backup="${new_backup##*$'\n'}"
  if [[ $new_backup != "$backup_dir"/agora-*.dump.enc || ! -f $new_backup ]]; then
    write_audit "failed"
    echo "replacement backup was not created in the expected directory" >&2
    exit 1
  fi
  while IFS= read -r old_backup; do
    if [[ $old_backup != "$new_backup" ]]; then
      if ! rm -f -- "$old_backup"; then
        write_audit "failed"
        echo "old backup cleanup failed; rerun the same command to resume cleanup" >&2
        exit 1
      fi
    fi
  done < <(find "$backup_dir" -maxdepth 1 -type f -name 'agora-*.dump.enc' -print)
  write_audit "completed"
else
  write_audit "not_requested"
fi

echo "$summary"
