#!/usr/bin/env python3
"""Local, secret-free five-minute operational observations."""
import datetime
import json
import os
from pathlib import Path
import shutil
import subprocess
import time

os.umask(0o077)
state = Path('/var/lib/agora-migration')
state.mkdir(mode=0o700, exist_ok=True)

def run(args):
    p = subprocess.run(args, capture_output=True, text=True, timeout=30)
    if p.returncode:
        raise RuntimeError('observation command failed: '+args[0])
    return p.stdout

now = time.time()
row = {'timestamp':datetime.datetime.now(datetime.timezone.utc).isoformat(), 'epoch':now}
try:
    pods = json.loads(run(['k3s','kubectl','get','pods','-n','agora','-o','json']))['items']
    row['pods'] = [{'name':p['metadata']['name'], 'phase':p['status'].get('phase'),
                    'ready':all(c.get('ready',False) for c in p['status'].get('containerStatuses',[])) and bool(p['status'].get('containerStatuses')),
                    'restarts':sum(c.get('restartCount',0) for c in p['status'].get('containerStatuses',[]))} for p in pods]
    row['ready'] = json.loads(run(['curl','-fsS','--max-time','10','https://agora.maese.com.ar/ready'])).get('status') == 'ready'
    row['queue'] = run(['k3s','kubectl','exec','-n','agora','postgres-0','--','psql','-U','postgres','-d','agora','-X','-Atc',"SELECT status||':'||count(*) FROM jobs GROUP BY status ORDER BY status;"]).strip().splitlines()
    row['disk_free_bytes'] = shutil.disk_usage('/').free
    mem = dict(line.split(':',1) for line in Path('/proc/meminfo').read_text().splitlines())
    row['memory_available_bytes'] = int(mem['MemAvailable'].split()[0])*1024
    row['load_average'] = [float(v) for v in Path('/proc/loadavg').read_text().split()[:3]]
    cpu = [int(v) for v in Path('/proc/stat').read_text().splitlines()[0].split()[1:9]]
    counters = {'total':sum(cpu),'idle':cpu[3]+cpu[4]}
    last = state/'cpu-last.json'
    if last.exists():
        previous=json.loads(last.read_text());delta=counters['total']-previous['total']
        if delta>0:row['cpu_percent']=round(100*(1-(counters['idle']-previous['idle'])/delta),2)
    last.write_text(json.dumps(counters))
    backups = list(Path('/var/backups/agora').glob('agora-*.dump.enc'))
    row['backup_age_seconds'] = now-max(p.stat().st_mtime for p in backups) if backups else None
    row['ok'] = (row['ready'] and len(row['pods'])==2 and all(p['ready'] for p in row['pods'])
                 and row['disk_free_bytes']>30*1024**3 and row['memory_available_bytes']>1024**3
                 and row['backup_age_seconds'] is not None and row['backup_age_seconds']<26*3600
                 and row.get('cpu_percent',0)<90)
except Exception as error:
    row['ok']=False
    row['error']=type(error).__name__
with Path('/var/log/agora-k8s-observation.jsonl').open('a') as log:
    log.write(json.dumps(row)+'\n')
print(json.dumps(row))
raise SystemExit(0 if row['ok'] else 1)
