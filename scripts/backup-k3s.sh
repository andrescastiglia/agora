#!/usr/bin/env bash
set -Eeuo pipefail
[[ $EUID == 0 ]] || exit 1
umask 077
backup_dir=/var/backups/agora-k3s
install -d -m 0700 "$backup_dir"
name="agora-$(date -u +%Y%m%dT%H%M%SZ)"
k3s etcd-snapshot save --name "$name" >/dev/null
snapshot="$(find /var/lib/rancher/k3s/server/db/snapshots -maxdepth 1 -name "$name-*" -type f -print -quit)"
test -n "$snapshot"
temporary="$backup_dir/$name.tar.enc.tmp"
trap 'rm -f "$temporary"' EXIT
tar -C / -cf - "${snapshot#/}" var/lib/rancher/k3s/server/token etc/rancher/k3s etc/agora/runtime.conf |
  openssl enc -aes-256-cbc -salt -pbkdf2 -pass file:/etc/agora/backup-passphrase -out "$temporary"
mv "$temporary" "$backup_dir/$name.tar.enc"
# Only this script's own snapshots; preserve cluster-wide snapshot policy.
rm -f -- "$snapshot"
find "$backup_dir" -name 'agora-*.tar.enc' -mtime +14 -delete
