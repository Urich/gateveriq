import {readFileSync} from 'node:fs';
import {runInNewContext} from 'node:vm';
import assert from 'node:assert/strict';

class Element {
  style = {}; attributes = {}; children = []; listeners = new Map();
  set textContent(value) { this.text = value; this.children = []; }
  setAttribute(name, value) { this.attributes[name] = value; }
  append(child) { this.children.push(child); }
  replaceChildren(...children) { this.children = children; }
  addEventListener(name, listener) { this.listeners.set(name, listener); }
  removeEventListener(name) { this.listeners.delete(name); }
  remove() {}
  click() { this.listeners.get('click')?.(); }
}
let terminated = 0, requests = 0;
const scope = {document: {createElement: () => new Element()}, Date, Error, Set, URL,
  AbortController, setTimeout, clearTimeout,
  Worker: class {
    postMessage() { queueMicrotask(() => this.onmessage({data: {nonce: 'proof'}})); }
    terminate() { terminated++; }
  },
  fetch: async (url, options) => {
    assert.equal(element.attributes['aria-busy'], 'true');
    assert.equal(element.children[0].className, 'gateveriq-loading');
    assert.equal(element.children[0].children[0].className, 'gateveriq-spinner');
    assert.equal(url.pathname, '/public/v1/web/visual/solutions');
    const body = JSON.parse(options.body);
    assert.equal(body.task_id, requests === 0 ? 'task' : 'replacement'); assert.equal(body.nonce, 'proof');
    requests++;
    return requests === 1
      ? {ok: false, json: async () => ({error: {code: 'INVALID_VISUAL_SOLUTION'}, visual: {...visual, task_id:'replacement',reference:'new-reference',max_attempts:2}})}
      : {ok: true, json: async () => ({verification_token: 'verified-token'})};
  }};
runInNewContext(readFileSync(new URL('../web/widget.js', import.meta.url), 'utf8')
  .replace(/^const workerURL.*$/m, "const workerURL = 'worker.js';").replaceAll('export ', ''), scope);
const visual = {task_id: 'task', instruction: 'Match shapes', reference: 'reference',
  images: Array(16).fill('image'), max_attempts: 3, expires_at: Date.now()/1000+60};
const element = new Element(), controller = new AbortController();
const selecting = scope.selectImages(element, visual, controller.signal);
const grid = element.children[2], confirm = element.children[4];
assert.equal(confirm.disabled, true);
grid.children[5].click(); grid.children[1].click(); grid.children[5].click();
assert.equal(grid.children[1].attributes['aria-pressed'], 'true');
confirm.click();
assert.deepEqual([...(await selecting)], [1]);
assert.equal(grid.children[1].listeners.size, 0);
const cancelled = scope.selectImages(element, visual, controller.signal);
controller.abort(); await assert.rejects(cancelled, /Cancelled/);

const verification = scope.verifyVisual(element, {challenge_id: 'challenge'}, visual,
  {serviceURL: 'https://verify.example'});
element.children[2].children[0].click(); element.children[4].click();
await new Promise(resolve => setImmediate(resolve));
assert.equal(element.children[1].src, 'new-reference');
assert.equal(element.children[3].text, 'Selection was incorrect. Try the new images.');
assert.equal(element.attributes['aria-busy'], 'false');
element.children[2].children[1].click(); element.children[4].click();
const result = await verification;
assert.equal(result.decision, 'verified'); assert.equal(result.verification_token, 'verified-token');
assert.equal(requests, 2); assert.equal(terminated, 1);
assert.equal(element.attributes['aria-busy'], 'false');
console.log('Visual selection, deselection, cancellation, retry and token completion passed');

scope.navigator = {languages: ['de-DE', 'ru-RU'], language: 'de-DE'};
assert.equal(scope.resolveLocale(element), 'ru');
scope.document.documentElement = {lang: 'en-GB'};
assert.equal(scope.resolveLocale(element), 'en');
assert.equal(scope.resolveLocale(element, 'ru-RU'), 'ru');
assert.equal(scope.resolveLocale(element, 'fr-FR'), 'en');
const russianController = new AbortController();
const russian = scope.selectImages(element, {...visual, instruction_key: 'match_reference'},
  russianController.signal, '', {locale: 'ru'});
assert.match(element.children[0].text, /^Выберите все картинки/);
assert.equal(element.children[1].alt, 'Образец для сравнения');
assert.equal(element.children[4].text, 'Подтвердить выбор');
assert.equal(element.children[2].children[0].attributes['aria-label'], 'Картинка 1');
russianController.abort();
await assert.rejects(russian, /Отменено/);
console.log('Visual localization: explicit locale, page language, browser preferences, fallback and Russian labels passed');
