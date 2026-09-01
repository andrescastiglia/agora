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

if [[ $backup_mode == "--replace-backups" ]]; then
  readonly backup_dir="${AGORA_BACKUP_DIR:-/var/backups/agora}"
  readonly backup_command="${AGORA_BACKUP_COMMAND:-/usr/local/sbin/agora-backup-postgres}"
  test -d "$backup_dir"
  new_backup="$($backup_command)"
  new_backup="${new_backup##*$'\n'}"
  if [[ $new_backup != "$backup_dir"/agora-*.dump.enc || ! -f $new_backup ]]; then
    echo "replacement backup was not created in the expected directory" >&2
    exit 1
  fi
  while IFS= read -r old_backup; do
    if [[ $old_backup != "$new_backup" ]]; then
      rm -f -- "$old_backup"
    fi
  done < <(find "$backup_dir" -maxdepth 1 -type f -name 'agora-*.dump.enc' -print)
fi

readonly audit_log="${AGORA_DELETION_AUDIT_LOG:-/var/log/agora-data-deletions.log}"
audit_dir="$(dirname "$audit_log")"
if [[ ! -d $audit_dir ]]; then
  install -d -m 0750 "$audit_dir"
fi
printf '%s %s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$summary" >>"$audit_log"
chmod 0600 "$audit_log"
echo "$summary"
