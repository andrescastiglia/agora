"""Exercise rollback and backend routing without touching Docker or a cluster."""
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
OLD = 'ghcr.io/andrescastiglia/agora@sha256:' + 'a' * 64
NEW = 'ghcr.io/andrescastiglia/agora@sha256:' + 'b' * 64

class DeployTests(unittest.TestCase):
    def exercise(self, backend, failure):
        with tempfile.TemporaryDirectory() as directory:
            d = Path(directory)
            (d / 'bin').mkdir()
            (d / 'k8s').mkdir()
            import shutil
            shutil.copytree(ROOT / 'k8s/app', d / 'k8s/app')
            (d / '.deployed-image').write_text(OLD + '\n')
            (d / 'runtime.conf').write_text('AGORA_RUNTIME_BACKEND=' + backend + '\n')
            stub = '''#!/usr/bin/env python3
import os,sys
from pathlib import Path
name=Path(sys.argv[0]).name
root=Path(os.environ['ORACLE_DEPLOY_PATH'])
with (root/'calls').open('a') as f:f.write(name+' '+' '.join(sys.argv[1:])+'\\n')
if name=='curl':
 p=root/'count'; n=int(p.read_text())+1 if p.exists() else 1;p.write_text(str(n))
 if os.environ['FAILURE']=='public':sys.exit(1 if 'https://' in ' '.join(sys.argv) else 0)
 sys.exit(1 if n <= (60 if os.environ['BACKEND']=='compose' else 1) else 0)
if name=='k3s' and 'apply' in sys.argv:
 import shutil
 shutil.copyfile(root/'k8s/app/deployment.yaml',root/'applied-app')
'''
            for name in ['docker', 'curl', 'k3s', 'sleep']:
                p = d / 'bin' / name
                p.write_text(stub)
                p.chmod(0o755)
            env = dict(os.environ, PATH=str(d/'bin')+':'+os.environ['PATH'],
                       ORACLE_DEPLOY_PATH=str(d), AGORA_RUNTIME_CONFIG=str(d/'runtime.conf'),
                       FAILURE=failure, BACKEND=backend)
            result = subprocess.run(['bash', str(ROOT/'scripts/deploy-oracle.sh'), NEW],
                                    env=env, capture_output=True, text=True)
            self.assertEqual(result.returncode, 1, result.stdout+result.stderr)
            self.assertEqual((d/'.deployed-image').read_text().strip(), OLD)
            self.assertIn('restoring previous', result.stderr)
            calls = (d/'calls').read_text()
            if backend == 'compose':
                self.assertIn('docker pull '+OLD, calls)
            else:
                self.assertEqual(calls.count('apply -k'), 2)
                self.assertNotIn('statefulset', calls)

    def test_compose_timeout_rolls_back(self): self.exercise('compose', 'timeout')
    def test_kubernetes_readiness_failure_rolls_back(self): self.exercise('kubernetes', 'timeout')
    def test_public_failure_rolls_back(self): self.exercise('kubernetes', 'public')

if __name__ == '__main__': unittest.main()
