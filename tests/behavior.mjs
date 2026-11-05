import {readFileSync} from 'node:fs';
import {runInNewContext} from 'node:vm';
import assert from 'node:assert/strict';
const source = readFileSync(new URL('../web/widget.js', import.meta.url), 'utf8')
  .replace(/^const workerURL.*$/m, '').replaceAll('export ', '');
class Target {
  listeners = new Map();
  children = [];
  addEventListener(type, fn) { this.listeners.set(type, fn); }
  removeEventListener(type) { this.listeners.delete(type); }
  emit(type, isTrusted = true) { this.listeners.get(type)?.({isTrusted}); }
  append(...children) { this.children.push(...children); }
  remove() {}
  replaceChildren(label) { this.label = label; }
}
let time = 0;
const scope = {performance: {now: () => time}, document: {createElement: () => new Target()},
  setTimeout, clearTimeout, Date, Error, Math};
runInNewContext(source, scope);
const element = new Target(), controller = new AbortController();
const challenge = {challenge_id: 'bound', expires_at: Date.now() / 1000 + 60};
for (const [delay, trusted, keyboard] of [[100, true, true], [500, true, false], [0, false, false]]) {
  const started = time;
  const pending = scope.collectBehavior(element, challenge, controller.signal);
  const checkbox = element.label.children[0];
  element.emit('keydown', false);
  if (keyboard) { element.emit('keydown'); element.emit('focusin'); }
  time = started + delay;
  checkbox.checked = true;
  checkbox.emit('change', trusted);
  const report = await pending;
  assert.equal(report.challenge_id, 'bound');
  assert.equal(report.keyboard_events, keyboard ? 1 : 0);
  assert.equal(report.pointer_events, 0);
  assert.equal(report.activation_ms, delay);
  assert.equal(report.trusted_activations, trusted ? 1 : 0);
  assert.equal(element.listeners.size, 0);
  assert.equal(checkbox.listeners.size, 0);
}
const cancelled = scope.collectBehavior(element, challenge, controller.signal);
controller.abort(); await assert.rejects(cancelled, /Cancelled/);
assert.equal(element.listeners.size, 0);
console.log('Behavior collection: quick activation, keyboard, synthetic events, cleanup and cancellation passed');
