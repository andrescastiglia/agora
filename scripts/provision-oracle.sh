#!/usr/bin/env bash
set -Eeuo pipefail

if [[ $EUID -ne 0 ]]; then
  echo "run this script as root" >&2
  exit 1
fi

install_config() {
  local destination="$1"
  local temporary
  temporary="$(mktemp)"
  cat >"$temporary"
  install -m 0644 "$temporary" "$destination"
  rm -f "$temporary"
}

install -d -o deploy -g deploy -m 0750 /opt/agora
install -d -o root -g deploy -m 0750 /etc/agora
install -d -o root -g root -m 0700 /var/backups/agora

if [[ ! -f /etc/agora/backup-passphrase ]]; then
  umask 077
  openssl rand -base64 48 >/etc/agora/backup-passphrase
fi

install_config /etc/nginx/conf.d/agora-log-format.conf <<'NGINX'
log_format agora_no_args '$remote_addr - $remote_user [$time_local] '
                         '"$request_method $uri $server_protocol" $status $body_bytes_sent '
                         '"$http_referer" "$http_user_agent"';
NGINX

if [[ ! -f /etc/nginx/sites-available/agora ]]; then
  install_config /etc/nginx/sites-available/agora <<'NGINX'
server {
    listen 80;
    listen [::]:80;
    server_name agora.maese.com.ar;

    # Query strings can contain Meta's webhook verification token.
    access_log /var/log/nginx/agora.access.log agora_no_args;

    client_max_body_size 1m;

    location / {
        proxy_pass http://127.0.0.1:8088;
        proxy_http_version 1.1;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
        proxy_connect_timeout 5s;
        proxy_read_timeout 95s;
        add_header X-Content-Type-Options nosniff always;
        add_header Referrer-Policy no-referrer always;
    }
}
NGINX
elif ! grep -qF 'access_log /var/log/nginx/agora.access.log agora_no_args;' \
  /etc/nginx/sites-available/agora; then
  sed -i \
    '/server_name agora\.maese\.com\.ar;/a\    access_log /var/log/nginx/agora.access.log agora_no_args;' \
    /etc/nginx/sites-available/agora
fi

ln -sfn /etc/nginx/sites-available/agora /etc/nginx/sites-enabled/agora
nginx -t
systemctl reload nginx

if [[ -f /opt/agora/backup-postgres.sh ]]; then
  install -o root -g root -m 0750 /opt/agora/backup-postgres.sh \
    /usr/local/sbin/agora-backup-postgres
  install -o root -g root -m 0750 /opt/agora/test-restore-postgres.sh \
    /usr/local/sbin/agora-test-restore-postgres
  install_config /etc/systemd/system/agora-backup.service <<'UNIT'
[Unit]
Description=Encrypted local PostgreSQL backup for Agora

[Service]
Type=oneshot
ExecStart=/usr/local/sbin/agora-backup-postgres
Nice=10
IOSchedulingClass=best-effort
IOSchedulingPriority=7
UNIT
  install_config /etc/systemd/system/agora-backup.timer <<'TIMER'
[Unit]
Description=Daily encrypted local PostgreSQL backup for Agora

[Timer]
OnCalendar=*-*-* 03:20:00
RandomizedDelaySec=20m
Persistent=true

[Install]
WantedBy=timers.target
TIMER
  systemctl daemon-reload
  systemctl enable --now agora-backup.timer
fi

echo "Nginx is ready. Obtain TLS with:"
echo "certbot --nginx -d agora.maese.com.ar --redirect"
