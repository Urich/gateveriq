import {readFileSync} from 'node:fs';
import {runInNewContext} from 'node:vm';
import assert from 'node:assert/strict';

class Element {
  attributes = {}; children = []; listeners = new Map(); disabled = false;
  style = {setProperty(name, value) { this[name] = value; }};
  constructor(tag = 'div') { this.tagName = tag; }
  set textContent(value) { this.text = value; this.children = []; }
  setAttribute(name, value) { this.attributes[name] = value; }
  append(...children) { this.children.push(...children); }
  replaceChildren(...children) { this.children = children; }
  addEventListener(name, listener) { this.listeners.set(name, listener); }
  removeEventListener(name) { this.listeners.delete(name); }
  remove() {}
  attachShadow() { this.shadowRoot = new Element('shadow-root'); return this.shadowRoot; }
  querySelectorAll(selector) {
    const matches = node => selector.split(',').some(part => part.startsWith('.')
      ? node.className === part.slice(1) : node.tagName === part);
    return this.children.flatMap(child => [ ...(matches(child) ? [child] : []), ...child.querySelectorAll(selector)]);
  }
  querySelector(selector) { return this.querySelectorAll(selector)[0]; }
  emit(name, event = {}) { this.listeners.get(name)?.({preventDefault() {}, ...event}); }
  set src(value) { this.source = value; queueMicrotask(() => this.emit('load')); }
  get src() { return this.source; }
}
let terminated = 0, requests = 0;
const scope = {document: {createElement: tag => new Element(tag)}, Date, Error, Set, URL,
  CSS: {supports: (_, value) => /^#[a-f0-9]{6}$|^\d+px$/i.test(value)},
  AbortController, setTimeout, clearTimeout,
  Worker: class {
    postMessage() { queueMicrotask(() => this.onmessage({data: {nonce: 'proof'}})); }
    terminate() { terminated++; }
  },
  fetch: async (url, options) => {
    const root = element.children[0].shadowRoot;
    assert.equal(root.querySelector('.card').attributes['aria-busy'], 'true');
    assert.equal(root.querySelector('input').disabled, true);
    assert.equal(url.pathname, '/public/v1/web/visual/solutions');
    const body = JSON.parse(options.body);
    assert.equal(body.task_id, requests === 0 ? 'task' : 'replacement');
    assert.equal(body.nonce, 'proof'); assert.equal(body.position, 100);
    assert.equal(body.selected, undefined);
    requests++;
    return requests === 1
      ? {ok: false, json: async () => ({error: {code: 'INVALID_VISUAL_SOLUTION'}, visual: {...visual, task_id:'replacement',background:'data:image/png;base64,new',max_attempts:2}})}
      : {ok: true, json: async () => ({verification_token: 'verified-token'})};
  }};
const source = ['slider.js','widget.js'].map(file => readFileSync(new URL('../web/'+file, import.meta.url), 'utf8')
  .replace(/^import .*$/gm, '').replace(/^const workerURL.*$/m, "const workerURL = 'worker.js';").replaceAll('export ', '')).join('\n');
runInNewContext(source, scope);
const visual = {kind:'slider_puzzle',task_id:'task',background:'data:image/png;base64,bg',piece:'data:image/png;base64,piece',
  width:280,height:140,piece_size:40,piece_y:30,max_position:240,max_attempts:3,expires_at:Date.now()/1000+60};
const element = new Element(), controller = new AbortController();
const tick = () => new Promise(resolve => setImmediate(resolve));
const root = () => element.children[0].shadowRoot;
const input = () => root().querySelector('input');
const selecting = scope.selectSlider(element, visual, controller.signal,'',{styleNonce:'test-nonce',theme:{preset:'dark',accent:'#aabbcc',radius:'10px',background:'url(bad)'}});
assert.equal(input().disabled, true);
assert.equal(root().children[0].nonce,'test-nonce');
await tick();
assert.equal(input().disabled, false);
assert.equal(element.children[0].attributes['data-theme'], 'dark');
assert.equal(element.children[0].style['--gateveriq-accent'], '#aabbcc');
assert.equal(element.children[0].style['--gateveriq-background'], undefined);
assert.equal(root().querySelector('button').disabled, true);
input().value = '100'; input().emit('input'); input().emit('change');
assert.equal(root().querySelector('button').disabled, false);
assert.equal(root().querySelector('.piece').style.left, `${100/280*100}%`);
// Arrow-key changes preview the position; Enter commits.
input().emit('keydown', {key:'Enter'});
assert.equal(await selecting, 100);
assert.equal(input().listeners.size, 0);
const cancelled = scope.selectSlider(element, visual, controller.signal);
controller.abort(); await assert.rejects(cancelled, /Cancelled/);
assert.equal(input().listeners.size, 0);
assert.equal(input().disabled,true);
await assert.rejects(scope.selectSlider(element, {...visual, kind:'other'}, new AbortController().signal), /Invalid slider/);
const expired = scope.selectSlider(element, {...visual, expires_at:0}, new AbortController().signal);
await assert.rejects(expired, /expired/);

const verification = scope.verifyVisual(element, {challenge_id:'challenge'}, visual,
  {serviceURL:'https://verify.example', theme:'dark'});
await tick();
input().value = '100'; input().emit('pointerdown'); input().emit('input'); input().emit('change');
await tick();
assert.equal(root().querySelector('.background').src,'data:image/png;base64,new');
assert.equal(root().querySelector('.note').text,'Try again with this new puzzle.');
assert.equal(element.children[0].attributes['data-theme'],'dark');
input().value = '100'; input().emit('input'); root().querySelector('button').emit('click');
const result = await verification;
assert.equal(result.decision,'verified'); assert.equal(result.verification_token,'verified-token');
assert.equal(requests,2); assert.equal(terminated,1);
assert.equal(root().querySelector('.card').attributes['data-state'],'success');
console.log('Slider keyboard/pointer, loading, themes, cancellation, expiry, retry and token completion passed');

scope.navigator = {languages:['de-DE','ru-RU'],language:'de-DE'};
assert.equal(scope.resolveLocale(element),'ru');
scope.document.documentElement = {lang:'en-GB'};
assert.equal(scope.resolveLocale(element),'en');
assert.equal(scope.resolveLocale(element,'ru-RU'),'ru');
assert.equal(scope.resolveLocale(element,'fr-FR'),'en');
const russianController = new AbortController();
const russian = scope.selectSlider(element,visual,russianController.signal,'',{locale:'ru'});
await tick();
assert.equal(root().querySelector('.heading').text,'Совместите фрагмент с вырезом');
assert.equal(input().attributes['aria-label'],'Положение фрагмента');
russianController.abort(); await assert.rejects(russian,/Отменено/);
console.log('Slider localization and fallback passed');
