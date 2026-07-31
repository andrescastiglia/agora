#!/usr/bin/env bash
set -Eeuo pipefail

readonly webhook_url="https://agora.maese.com.ar/webhooks/telegram"

mode="apply"
if [[ ${1:-} == "--check" ]]; then
  mode="check"
  shift
fi
if [[ $# -gt 1 ]]; then
  echo "usage: configure-telegram-webhook.sh [--check] [env-file]" >&2
  exit 2
fi
readonly mode
readonly env_file="${1:-/etc/agora/agora.env}"

if [[ ! -r "$env_file" ]]; then
  echo "Telegram environment file is not readable: $env_file" >&2
  exit 1
fi

telegram_bot_token=""
telegram_webhook_secret=""
while IFS='=' read -r key value; do
  value="${value%$'\r'}"
  case "$key" in
    TELEGRAM_BOT_TOKEN) telegram_bot_token="$value" ;;
    TELEGRAM_WEBHOOK_SECRET) telegram_webhook_secret="$value" ;;
  esac
done <"$env_file"
readonly telegram_bot_token telegram_webhook_secret

if [[ ! $telegram_bot_token =~ ^[0-9]+:[A-Za-z0-9_-]+$ ]]; then
  echo "TELEGRAM_BOT_TOKEN is missing or malformed" >&2
  exit 1
fi
if [[ ! $telegram_webhook_secret =~ ^[A-Za-z0-9_-]{1,256}$ ]]; then
  echo "TELEGRAM_WEBHOOK_SECRET is missing or malformed" >&2
  exit 1
fi

telegram_request() {
  local method="$1"
  local data="${2:-}"
  {
    printf 'url = "https://api.telegram.org/bot%s/%s"\n' \
      "$telegram_bot_token" "$method"
    printf 'request = "POST"\n'
    printf 'header = "Content-Type: application/json"\n'
    if [[ -n $data ]]; then
      printf 'data = "%s"\n' "$data"
    fi
  } | curl --silent --show-error --fail-with-body --config -
}

if [[ $mode == "apply" ]]; then
  escaped_data=$(printf \
    '{\\"url\\":\\"%s\\",\\"secret_token\\":\\"%s\\",\\"allowed_updates\\":[\\"message\\",\\"edited_message\\"],\\"drop_pending_updates\\":false}' \
    "$webhook_url" "$telegram_webhook_secret")
  readonly escaped_data
  response=$(telegram_request "setWebhook" "$escaped_data")
  if ! grep -Eq '"ok"[[:space:]]*:[[:space:]]*true' <<<"$response"; then
    echo "Telegram rejected the webhook configuration" >&2
    exit 1
  fi
fi

response=$(telegram_request "getWebhookInfo")
if ! grep -Eq "\"url\"[[:space:]]*:[[:space:]]*\"$webhook_url\"" \
  <<<"$response"; then
  echo "Telegram webhook does not point to the expected Agora URL" >&2
  exit 1
fi
if ! grep -Eq '"ok"[[:space:]]*:[[:space:]]*true' <<<"$response"; then
  echo "Telegram webhook status could not be verified" >&2
  exit 1
fi

if [[ $mode == "apply" ]]; then
  echo "Telegram webhook configured and verified"
else
  echo "Telegram webhook verified"
fi
