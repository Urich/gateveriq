"""Real listener/CLI smoke test. Run after cargo build: python3 tests/e2e.py."""
import json
import os
from pathlib import Path
import socket
import subprocess
import tempfile
import time
import urllib.request
import urllib.error

BINARY = (Path(__file__).resolve().parents[1] / 'target/debug' / ('gateveriq.exe' if os.name == 'nt' else 'gateveriq')).resolve()

def port():
    with socket.socket() as sock:
        sock.bind(('127.0.0.1', 0))
        return sock.getsockname()[1]

with tempfile.TemporaryDirectory() as directory:
    def cli(*args):
        return subprocess.check_output([str(BINARY), *args], cwd=directory, text=True)
    cli('init')
    config = Path(directory)/'config.toml'
    public, internal = port(), port()
    while public == internal:
        internal = port()
    config.write_text(config.read_text().replace('127.0.0.1:9080', f'127.0.0.1:{public}').replace('127.0.0.1:9081', f'127.0.0.1:{internal}').replace('difficulty_bits = 18', 'difficulty_bits = 8'))
    integration = json.loads(cli('integration-create', '--name', 'e2e', '--actions', 'test'))
    def post(port, path, data, auth=False):
        headers = {'Content-Type':'application/json'}
        if auth:
            headers['Authorization']='Bearer '+integration['integration_secret']
        req = urllib.request.Request(f'http://127.0.0.1:{port}{path}', json.dumps(data).encode(), headers)
        with urllib.request.urlopen(req, timeout=5) as response:
            return json.load(response)
    def launch():
        process = subprocess.Popen([str(BINARY),'serve'],cwd=directory)
        for _ in range(100):
            try:
                urllib.request.urlopen(f'http://127.0.0.1:{public}/health',timeout=.2).close()
                return process
            except OSError:
                time.sleep(.05)
        process.terminate();process.wait();raise RuntimeError('Server did not start')
    process=launch()
    try:
        prepared={'integration_id':integration['integration_id'],'action':'test','context_id':'e2e-context'}
        challenge=post(internal,'/internal/v1/challenges',prepared,True)
        assert challenge == post(internal,'/internal/v1/challenges',prepared,True)
        file=Path(directory)/'challenge.json';file.write_text(json.dumps(challenge))
        solved=json.loads(cli('solve','--challenge',str(file),'--submit-url',f'http://127.0.0.1:{public}/public/v1/solutions'))
        process.terminate();process.wait();process=launch()
        request={**prepared,'verification_token':solved['verification_token'],'request_id':'consume-1'}
        assert post(internal,'/internal/v1/verifications/consume',request,True)['valid']
        assert post(internal,'/internal/v1/verifications/consume',request,True)['valid']
        try:
            post(internal,'/internal/v1/verifications/consume',{**request,'request_id':'consume-2'},True)
            raise AssertionError('Replay accepted')
        except urllib.error.HTTPError as error:
            assert json.load(error)['error']['code']=='TOKEN_ALREADY_CONSUMED'
        print('HTTP/CLI end-to-end and restart test passed')
    finally:
        process.terminate();process.wait()
