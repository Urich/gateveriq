import {readFileSync} from 'node:fs';
import {runInNewContext} from 'node:vm';
import assert from 'node:assert/strict';
const source = readFileSync(new URL('../web/widget.js', import.meta.url), 'utf8')
  .replace(/^const workerURL.*$/m, '').replaceAll('export ', '');
class Target {
  listeners = new Map();
  addEventListener(type, fn) { this.listeners.set(type, fn); }
  removeEventListener(type) { this.listeners.delete(type); }
  emit(type, isTrusted = true) { this.listeners.get(type)?.({isTrusted}); }
  remove() {}
  replaceChildren(button) { this.button = button; }
}
let time = 0;
const scope = {performance: {now: () => time}, document: {createElement: () => new Target()},
  setTimeout, clearTimeout, Date, Error, Math};
runInNewContext(source, scope);
const element = new Target(), controller = new AbortController();
const challenge = {challenge_id: 'bound', expires_at: Date.now() / 1000 + 60};
const pending = scope.collectBehavior(element, challenge, controller.signal);
element.emit('keydown', false);
assert.equal(element.button.listeners.has('click'), true);
time = 100; element.button.emit('click');
assert.equal(element.button.listeners.has('click'), true);
time = 500; element.button.emit('click', false);
assert.equal(element.button.listeners.has('click'), true);
element.emit('keydown'); element.emit('focusin'); element.button.emit('click');
const report = await pending;
assert.equal(report.challenge_id, 'bound');
assert.equal(report.keyboard_events, 1); assert.equal(report.pointer_events, 0);
assert.equal(report.activation_ms, 500); assert.equal(report.trusted_activations, 1);
assert.equal(element.listeners.size, 0);
const cancelled = scope.collectBehavior(element, challenge, controller.signal);
controller.abort(); await assert.rejects(cancelled, /Cancelled/);
assert.equal(element.listeners.size, 0);
console.log('Behavior collection: keyboard, synthetic events, timing, cancellation passed');
