import json
import os
from pathlib import Path
import subprocess
import tempfile
import time

binary = Path('target/ci/anda' + ('.exe' if os.name == 'nt' else '')).resolve()
source = json.dumps({'op': 'exec', 'source': 'return 1'}).encode()
request = b'\x01' + len(source).to_bytes(4, 'big') + source
minimal = {'MIMALLOC_DISALLOW_ARENA_ALLOC': '1'}

def probe(label, env, trace=False):
    with tempfile.TemporaryDirectory() as workdir:
        command = [str(binary), '--internal-code-mode-worker']
        trace_path = Path(workdir) / 'worker.trace'
        if trace:
            command = ['strace', '-f', '-e', 'trace=%memory,prlimit64,prctl',
                       '-o', str(trace_path), '--', *command]
        with subprocess.Popen(command, cwd=workdir, env=env, stdin=subprocess.PIPE,
                              stdout=subprocess.PIPE, stderr=subprocess.PIPE) as worker:
            if os.name != 'nt' and not trace:
                time.sleep(0.2)
                status = Path(f'/proc/{worker.pid}/status')
                if status.exists():
                    print('\n'.join(line for line in status.read_text().splitlines()
                                    if line.startswith(('VmSize:', 'VmRSS:', 'VmPeak:'))))
            try:
                stdout, stderr = worker.communicate(request, timeout=25)
            except subprocess.TimeoutExpired:
                worker.kill()
                stdout, stderr = worker.communicate()
            print(label, 'exit', worker.returncode, 'stdout', repr(stdout[:1024]))
            if stderr:
                print(stderr[:2048].decode(errors='replace'))
        if trace_path.exists():
            print('\n'.join(trace_path.read_text(errors='replace').splitlines()[-60:]))
        return worker.returncode == 0 and b'"op":"done"' in stdout

baseline = probe('baseline', minimal)
if os.name == 'nt':
    os_paths = {name: os.environ[name] for name in
                ('SystemRoot', 'LOCALAPPDATA', 'USERPROFILE') if name in os.environ}
    probe('bootstrap OS environment', minimal | os_paths)
    probe('profile', minimal | {'ANDA_CI_PROFILE_PROBE': '1'})
    probe('profile and bootstrap OS environment', minimal | os_paths | {'ANDA_CI_PROFILE_PROBE': '1'})
elif not baseline:
    probe('syscall trace', minimal | {'MIMALLOC_VERBOSE': '1'}, trace=True)
raise SystemExit(not baseline)
