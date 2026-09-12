#!/usr/bin/env python3
"""Run as root on oracle. Never serialize credentials to output or Git."""
import json
import os
from pathlib import Path
import secrets
import shlex
import subprocess
import sys

if os.geteuid() != 0:
    raise SystemExit('run as root')
if sys.argv[1:] == ['--postgres-admin']:
    p = Path('/etc/agora/postgres-admin-password')
    if not p.exists():
        descriptor = os.open(p, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
        with os.fdopen(descriptor, 'w') as stream:
            stream.write(secrets.token_urlsafe(48))
    obj = {'apiVersion':'v1','kind':'Secret',
           'metadata':{'name':'postgres-admin','namespace':'agora'},
           'type':'Opaque','stringData':{'password':p.read_text().strip()}}
    result = subprocess.run(['k3s','kubectl','apply','--server-side','-f','-'],
                            input=json.dumps(obj),text=True,capture_output=True)
    if result.returncode:
        raise SystemExit('failed to provision PostgreSQL admin Secret; details suppressed')
    print('PostgreSQL admin Secret is ready; no credentials emitted')
    raise SystemExit(0)
if sys.argv[1:] not in [[], ['--application']]:
    raise SystemExit('usage: configure-k8s-secrets.py --postgres-admin|--application')
values = {}
for line in Path('/etc/agora/agora.env').read_text().splitlines():
    if not line.strip() or line.lstrip().startswith('#'):
        continue
    key, value = line.removeprefix('export ').split('=', 1)
    parts = shlex.split(value, comments=True)
    values[key.strip()] = parts[0] if parts else ''
public = {
    'CHAT_PROVIDER', 'KNOWLEDGE_SPACE_ID', 'BOT_MENTION', 'META_GRAPH_API_VERSION',
    'OPENAI_EMBEDDING_MODEL', 'OPENAI_EMBEDDING_DIMENSIONS', 'OPENAI_RESPONSE_MODEL',
    'OPENAI_BASE_URL', 'RUST_LOG', 'WORKER_POLL_INTERVAL_MS',
    'DOCUMENT_MAX_BYTES', 'WEBHOOK_MAX_BODY_BYTES', 'TELEGRAM_BOT_USERNAME',
}
private = {
    'OPENAI_API_KEY', 'TELEGRAM_BOT_TOKEN', 'TELEGRAM_WEBHOOK_SECRET',
    'TELEGRAM_GROUP_ID', 'TELEGRAM_ALLOWED_USER_IDS', 'WHATSAPP_VERIFY_TOKEN',
    'WHATSAPP_APP_SECRET', 'WHATSAPP_ACCESS_TOKEN', 'WHATSAPP_PHONE_NUMBER_ID',
    'WHATSAPP_WABA_ID', 'WHATSAPP_GROUP_ID', 'WHATSAPP_ALLOWED_USER_IDS', 'ALLOWED_WHATSAPP_IDS',
}
p = Path('/etc/agora/k8s-app-password')
if not p.exists():
    descriptor = os.open(p, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    with os.fdopen(descriptor, 'w') as stream:
        stream.write(secrets.token_hex(32))
password = p.read_text().strip()
if len(password) != 64 or any(c not in '0123456789abcdef' for c in password):
    raise SystemExit('invalid protected password file')
sql = "DO $$ BEGIN IF NOT EXISTS (SELECT FROM pg_roles WHERE rolname='agora') THEN CREATE ROLE agora; END IF; END $$;\n"
sql += f"ALTER ROLE agora LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE PASSWORD '{password}';\n"
result = subprocess.run(['k3s','kubectl','exec','-i','-n','agora','postgres-0','--',
                         'psql','-U','postgres','-X','-v','ON_ERROR_STOP=1'], input=sql,
                        text=True, capture_output=True)
if result.returncode:
    raise SystemExit('failed to configure database role; details suppressed to protect credentials')
secret_values = {k:v for k,v in values.items() if k in private}
secret_values['DATABASE_URL'] = f'postgres://agora:{password}@postgres.agora.svc.cluster.local:5432/agora'
for kind, name, field, data in [
    ('ConfigMap','agora-config','data',{k:v for k,v in values.items() if k in public}),
    ('Secret','agora-runtime','stringData',secret_values),
]:
    obj = {'apiVersion':'v1','kind':kind,'metadata':{'name':name,'namespace':'agora'},field:data}
    result = subprocess.run(['k3s','kubectl','apply','--server-side','-f','-'],
                            input=json.dumps(obj),text=True,capture_output=True)
    if result.returncode:
        raise SystemExit('failed to apply protected configuration; details suppressed')
print('Application configuration and database role are ready; no credentials emitted')
