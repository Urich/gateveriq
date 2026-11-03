import assert from 'node:assert/strict';
import {GateveriqClient} from '../sdk/server.mjs';
import {readFileSync} from 'node:fs';
import {runInNewContext} from 'node:vm';

const calls = [];
const client = new GateveriqClient({integrationId: 'integration', secret: 'backend-only',
  fetchImpl: async (url, options) => {
    const body = JSON.parse(options.body);
    assert.equal(body.integration_id, 'integration');
    assert.equal(options.headers.Authorization, 'Bearer backend-only');
    calls.push({url: url.pathname, body});
    return {ok: true, json: async () => ({decision: 'verify', valid: true, recorded: true})};
  }});
const pending = await client.prepareLogin({accountId: 'opaque', origin: 'https://site.example'});
assert.match(pending.contextId, /^[\w-]{43}$/);
assert.notEqual(pending.contextId, pending.requestId);
await client.consume({...pending, verificationToken: 'token'});
await client.reportOutcome({...pending, success: false});
assert.equal(calls[1].body.context_id, calls[0].body.context_id);
assert.equal(calls[2].body.request_id, calls[1].body.request_id);
await assert.rejects(client.reportOutcome({...pending, success: 'false'}), /boolean/);
const failing = new GateveriqClient({fetchImpl: async () => ({ok: false, status: 403,
  json: async () => ({error: {code: 'UNAUTHORIZED'}})})});
await assert.rejects(failing.prepareLogin({}), error => error.code === 'UNAUTHORIZED');

const scope = {};
runInNewContext(readFileSync(new URL('../web/widget.js', import.meta.url), 'utf8')
  .replace(/^const workerURL.*$/m, '').replaceAll('export ', ''), scope);
const result = await scope.verifyWebsiteAttempt(null, {decision: 'additional_verification_required'});
assert.equal(result.decision, 'additional_verification_required');
assert.equal(result.verification_token, undefined);
await assert.rejects(scope.verifyWebsiteAttempt(null, {decision: 'unexpected'}), /Invalid website attempt/);
console.log('Backend SDK binding, error propagation and pending additional check passed');
