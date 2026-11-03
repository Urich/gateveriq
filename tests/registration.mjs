import assert from 'node:assert/strict';
import {RegistrationFlow} from '../examples/registration.mjs';
let consumes = 0;
let sequence = 0;
const gate = {
  createAttempt: data => ({...data, contextId:`c${++sequence}`, requestId:`r${sequence}`}),
  prepareAttempt: async pending => ({attempt:{challenge:pending.contextId}}),
  consume: async () => { consumes++; return {valid:true, verification_method:'pow_visual'}; },
  reportOutcome: async () => ({recorded:true}),
};
const flow = new RegistrationFlow(gate);
const id = flow.create('owner', 'opaque', 'https://site.example');
assert.deepEqual(await flow.prepare('owner', id), await flow.prepare('owner', id));
await assert.rejects(flow.prepare('intruder', id), /Unknown operation/);
const payload = {demoAnswer:'accepted'};
const results = await Promise.all([flow.complete('owner', id, 'token', payload), flow.complete('owner', id, 'token', payload)]);
assert.deepEqual(results[0], results[1]);
assert.equal(consumes, 1);
assert.throws(() => flow.complete('owner', id, 'token', {demoAnswer:'changed'}), /conflict/);
const retry = flow.create('owner', 'opaque', 'https://site.example', 'email_verify');
assert.equal((await flow.complete('owner', retry, 'new-token', {demoAnswer:'wrong'})).success, false);
console.log('Registration ownership, immutable requests and once-only completion passed');
