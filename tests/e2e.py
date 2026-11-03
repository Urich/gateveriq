"""Real listener/CLI smoke test. Run after cargo build: python3 tests/e2e.py."""
import json
from contextlib import closing
import os
from pathlib import Path
import socket
import secrets
import base64
import sqlite3
import struct
import zlib
import subprocess
import tempfile
import time
import urllib.request
import urllib.error

BINARY = Path(os.environ['GATEVERIQ_BINARY']).resolve() if os.environ.get('GATEVERIQ_BINARY') else (Path(__file__).resolve().parents[1] / 'target/debug' / ('gateveriq.exe' if os.name == 'nt' else 'gateveriq')).resolve()

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
    config.write_text(config.read_text().replace('127.0.0.1:9080', f'127.0.0.1:{public}').replace('127.0.0.1:9081', f'127.0.0.1:{internal}').replace('difficulty_bits = 18', 'difficulty_bits = 8').replace('account_failure_limit = 5', 'account_failure_limit = 1'))
    integration = json.loads(cli('integration-create', '--name', 'e2e', '--actions', 'test'))
    def post(port, path, data, auth=False, origin=None):
        headers = {'Content-Type':'application/json'}
        if origin:
            headers['Origin'] = origin
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
        integration = json.loads(cli('integration-create', '--name', 'website', '--actions', 'login', '--origins', 'http://127.0.0.1:8090'))
        context = secrets.token_urlsafe(32)
        attempt = {'integration_id':integration['integration_id'], 'action':'login', 'context_id':context, 'account_id':'opaque-account', 'origin':'http://127.0.0.1:8090'}
        prepared_web = post(internal, '/internal/v1/web/attempts', attempt, True)
        assert prepared_web['decision'] == 'verify'
        challenge = prepared_web['challenge']
        file.write_text(json.dumps(challenge))
        solution = json.loads(cli('solve', '--challenge', str(file)))
        solution['browser'] = {'webdriver':False, 'cookies_enabled':True, 'languages_count':1}
        solution['behavior'] = {'challenge_id':challenge['challenge_id'], 'elapsed_ms':1000, 'activation_ms':500, 'trusted_activations':1, 'pointer_events':0, 'keyboard_events':1, 'focus_changes':1}
        process.terminate();process.wait();process=launch()
        verified = post(public, '/public/v1/web/solutions', solution, origin='http://127.0.0.1:8090')
        consume = {'integration_id':integration['integration_id'], 'action':'login', 'context_id':context, 'request_id':'website-login', 'verification_token':verified['verification_token']}
        assert post(internal, '/internal/v1/verifications/consume', consume, True)['valid']
        outcome = {key:consume[key] for key in ['integration_id','action','context_id','request_id']}
        outcome['success'] = False
        assert post(internal, '/internal/v1/web/outcomes', outcome, True)['recorded']
        assert post(internal, '/internal/v1/web/outcomes', outcome, True)['recorded']
        attempt['context_id'] = secrets.token_urlsafe(32)
        extra = post(internal, '/internal/v1/web/attempts', attempt, True)
        assert extra['decision'] == 'additional_verification_required' and extra['verification_token'] is None
        visual = extra['visual']
        assert len(visual['images']) == 16 and 'answer' not in visual
        for encoded in [visual['reference'], *visual['images']]:
            png = base64.b64decode(encoded.split(',', 1)[1])
            assert png[:8] == b'\x89PNG\r\n\x1a\n'
            offset = 8
            compressed = bytearray()
            width = height = 0
            while offset < len(png):
                size = struct.unpack('>I', png[offset:offset+4])[0]
                kind = png[offset+4:offset+8]
                data = png[offset+8:offset+8+size]
                crc = struct.unpack('>I', png[offset+8+size:offset+12+size])[0]
                assert zlib.crc32(kind+data) == crc
                if kind == b'IHDR':
                    width, height = struct.unpack('>II', data[:8])
                    assert (width, height) == (160, 160)
                    assert data[8:10] == bytes([8, 2])
                if kind == b'IDAT':
                    compressed.extend(data)
                offset += 12+size
            raw = zlib.decompress(compressed)
            assert len(raw) == height*(width*3+1)
            assert all(raw[y*(width*3+1)] <= 4 for y in range(height))
        # Privileged test harness reads the server answer; the public response never includes it.
        with closing(sqlite3.connect(Path(directory)/'gateveriq.sqlite3')) as db:
            mask = db.execute('SELECT answer FROM visual_tasks WHERE task_id=?', [visual['task_id']]).fetchone()[0]
        file.write_text(json.dumps(extra['challenge']))
        proof = json.loads(cli('solve', '--challenge', str(file)))
        proof.update(task_id=visual['task_id'], selected=[])
        try:
            post(public, '/public/v1/web/visual/solutions', proof, origin='http://127.0.0.1:8090')
            raise AssertionError('Incorrect visual selection accepted')
        except urllib.error.HTTPError as error:
            assert error.code == 400
            replacement = json.load(error)
        assert replacement['error']['code'] == 'INVALID_VISUAL_SOLUTION'
        rotated = replacement['visual']
        assert rotated['task_id'] != visual['task_id']
        assert rotated['reference'] != visual['reference']
        assert rotated['expires_at'] == visual['expires_at'] and rotated['max_attempts'] == 2
        visual = rotated
        with closing(sqlite3.connect(Path(directory)/'gateveriq.sqlite3')) as db:
            mask = db.execute('SELECT answer FROM visual_tasks WHERE task_id=?', [visual['task_id']]).fetchone()[0]
        proof.update(task_id=visual['task_id'], selected=[i for i in range(16) if mask & (1 << i)])
        verified = post(public, '/public/v1/web/visual/solutions', proof, origin='http://127.0.0.1:8090')
        consume.update(context_id=attempt['context_id'], request_id='visual-login', verification_token=verified['verification_token'])
        assert post(internal, '/internal/v1/verifications/consume', consume, True)['valid']
        print('HTTP/CLI, website login, visual completion, PNG decoding, risk and restart tests passed')
    finally:
        process.terminate();process.wait()
