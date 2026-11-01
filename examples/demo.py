#!/usr/bin/env python3
"""Local-only integration example. Never deploy this demo as an application backend."""
import json
import os
import secrets
import threading
import time
import urllib.error
import urllib.request
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
INTEGRATION = os.environ['GATEVERIQ_ID']
SECRET = os.environ['GATEVERIQ_SECRET']
OPERATIONS = {}
LOCK = threading.Lock()

def api(path, body, internal=False):
    headers = {'Content-Type': 'application/json'}
    if internal:
        headers['Authorization'] = 'Bearer ' + SECRET
    port = 9081 if internal else 9080
    req = urllib.request.Request(f'http://127.0.0.1:{port}{path}', json.dumps(body).encode(), headers)
    with urllib.request.urlopen(req, timeout=10) as response:
        return json.load(response)

class Handler(BaseHTTPRequestHandler):
    def log_message(self, *args):
        pass  # Avoid logging operation IDs and tokens.

    def send(self, status, body, content_type='application/json'):
        raw = body if isinstance(body, bytes) else json.dumps(body).encode()
        self.send_response(status)
        self.send_header('Content-Type', content_type)
        self.send_header('Cache-Control', 'no-store')
        self.send_header('Content-Length', str(len(raw)))
        self.end_headers()
        self.wfile.write(raw)

    def do_GET(self):
        files = {'/': ('examples/demo.html', 'text/html'),
                 '/widget.js': ('web/widget.js', 'text/javascript'),
                 '/worker.js': ('web/worker.js', 'text/javascript')}
        if self.path not in files:
            self.send(404, {'error': 'Not found'})
            return
        file, mime = files[self.path]
        self.send(200, (ROOT / file).read_bytes(), mime)

    def do_POST(self):
        # Exact-origin check prevents other websites from exercising the local demo.
        if self.headers.get('Origin') != 'http://127.0.0.1:8090':
            self.send(403, {'error': 'Invalid origin'})
            return
        try:
            length = int(self.headers.get('Content-Length', '0'))
            if not 0 <= length <= 4096:
                self.send(413, {'error': 'Body too large'})
                return
            data = json.loads(self.rfile.read(length) or b'{}')
            if self.path == '/prepare':
                operation = secrets.token_urlsafe(32)
                challenge = api('/internal/v1/challenges', {
                    'integration_id': INTEGRATION, 'action': 'demo', 'context_id': operation}, True)
                with LOCK:
                    now = time.time()
                    for key in list(OPERATIONS):
                        if OPERATIONS[key]['expires'] < now:
                            del OPERATIONS[key]
                    if len(OPERATIONS) >= 100:
                        self.send(429, {'error': 'Demo operation limit'})
                        return
                    OPERATIONS[operation] = {'expires': now + 300, 'request_id': secrets.token_urlsafe(32), 'done': False}
                self.send(200, {'operation_id': operation, 'challenge': challenge})
            elif self.path == '/public/v1/solutions':
                self.send(200, api(self.path, data))
            elif self.path == '/complete':
                operation = data['operation_id']
                with LOCK:
                    record = OPERATIONS.get(operation)
                    if not record or record['expires'] < time.time():
                        self.send(400, {'error': 'Unknown or expired operation'})
                        return
                    if record['done']:
                        self.send(200, {'message': 'Operation already completed'})
                        return
                    api('/internal/v1/verifications/consume', {
                        'integration_id': INTEGRATION, 'action': 'demo', 'context_id': operation,
                        'request_id': record['request_id'], 'verification_token': data['verification_token']}, True)
                    record['done'] = True
                self.send(200, {'message': 'Verified: demo operation completed'})
            else:
                self.send(404, {'error': 'Not found'})
        except urllib.error.HTTPError as error:
            self.send(error.code, error.read(), error.headers.get('Content-Type', 'text/plain'))
        except (ValueError, KeyError, TypeError):
            self.send(400, {'error': 'Invalid request'})
        except Exception:
            self.send(503, {'error': 'Backend unavailable'})

if __name__ == '__main__':
    print('Local demo: http://127.0.0.1:8090 (action "demo" must be allowed)')
    ThreadingHTTPServer(('127.0.0.1', 8090), Handler).serve_forever()
