#!/usr/bin/env bash
set -Eeuo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/runtime-common.sh"

if [[ $# -ne 3 ]]; then
  echo "usage: export-participant-data.sh <telegram|whatsapp> <participant-id> <output.json>" >&2
  exit 2
fi

readonly provider="$1"
readonly participant_id="$2"
readonly destination="$3"
readonly script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
readonly database_name="${AGORA_DATABASE_NAME:-agora}"

if [[ $provider != "telegram" && $provider != "whatsapp" ]]; then
  echo "provider must be telegram or whatsapp" >&2
  exit 2
fi
if [[ ! $participant_id =~ ^[A-Za-z0-9_.:+-]+$ ]]; then
  echo "participant-id contains unsupported characters" >&2
  exit 2
fi
if [[ -e $destination ]]; then
  echo "output already exists: $destination" >&2
  exit 1
fi
if [[ ! -d $(dirname "$destination") ]]; then
  echo "output directory does not exist" >&2
  exit 1
fi

psql_command=(agora_psql)

umask 077
temporary="$(mktemp "${destination}.tmp.XXXXXX")"
trap 'rm -f "$temporary"' EXIT

"${psql_command[@]}" --no-psqlrc --set ON_ERROR_STOP=1 --quiet --tuples-only --no-align \
  < <(
    printf "SET agora.provider = '%s'; SET agora.participant_id = '%s';\n" \
      "$provider" "$participant_id"
    sed -n '1,$p' "$script_dir/export-participant-data.sql"
  ) >"$temporary"

test -s "$temporary"
mv "$temporary" "$destination"
trap - EXIT
echo "Participant export written with mode 0600: $destination"
