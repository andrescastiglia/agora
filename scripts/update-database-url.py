#!/usr/bin/env python3
"""Validated, atomic rollback endpoint updates; credentials never reach output."""
import os
from pathlib import Path
import re
import shlex
import sys
import tempfile
from urllib.parse import urlsplit, urlunsplit


def assignments(contents, key):
    pattern = re.compile(r'^(\s*(?:export\s+)?'+re.escape(key)+r'\s*=\s*)(.*)$')
    matches = []
    for number, line in enumerate(contents.splitlines()):
        match = pattern.match(line)
        if match:
            parts = shlex.split(match.group(2), comments=True)
            if len(parts) != 1:
                raise ValueError('invalid environment assignment')
            matches.append((number, match.group(1), parts[0]))
    return matches


def rewrite_database_url(contents, database):
    if not re.fullmatch(r'agora_rollback_[0-9]{14}', database):
        raise ValueError('invalid rollback database name')
    matches = assignments(contents, 'DATABASE_URL')
    if len(matches) != 1:
        raise ValueError('expected exactly one DATABASE_URL assignment')
    number, prefix, value = matches[0]
    url = urlsplit(value)
    if url.scheme not in ('postgres', 'postgresql') or url.hostname not in ('127.0.0.1', 'localhost'):
        raise ValueError('rollback requires the original host database endpoint')
    lines = contents.splitlines()
    lines[number] = prefix + shlex.quote(urlunsplit(url._replace(path='/'+database)))
    return '\n'.join(lines)+'\n'


def main():
    if os.geteuid() != 0:
        raise ValueError('run as root')
    if len(sys.argv) == 3 and sys.argv[1] == '--provider':
        matches = assignments(Path(sys.argv[2]).read_text(), 'CHAT_PROVIDER')
        if len(matches) > 1:
            raise ValueError('duplicate provider assignment')
        provider = matches[0][2] if matches else 'telegram'
        if provider not in ('telegram', 'whatsapp'):
            raise ValueError('invalid active provider')
        print(provider)
        return
    if len(sys.argv) != 3:
        raise ValueError('usage: update-database-url.py env-file rollback-database')
    path = Path(sys.argv[1])
    updated = rewrite_database_url(path.read_text(), sys.argv[2])
    metadata = path.stat()
    fd, temporary = tempfile.mkstemp(prefix='.agora-env-', dir=path.parent)
    try:
        with os.fdopen(fd, 'w') as stream:
            os.fchmod(stream.fileno(), metadata.st_mode & 0o777)
            os.fchown(stream.fileno(), metadata.st_uid, metadata.st_gid)
            stream.write(updated)
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(temporary, path)
    finally:
        if os.path.exists(temporary):
            os.unlink(temporary)
    print('Rollback database endpoint updated')


if __name__ == '__main__':
    try:
        main()
    except Exception:
        # Do not print parser/URL errors that could include credentials.
        raise SystemExit('database environment validation/update failed; credentials suppressed') from None
