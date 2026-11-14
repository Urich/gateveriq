import {sliderRoot, sliderState} from './slider.js';
const workerURL = new URL('./worker.js', import.meta.url);
const visualMessages = {
  en: {
    instruction: 'Fit the puzzle piece', drag: 'Slide to align', position: 'Puzzle piece position',
    loading: 'Loading puzzle…', ready: 'Release or press Enter',
    confirm: 'Check',
    checking: 'Checking puzzle…', completed: 'Verification completed',
    retry: 'Try again with this new puzzle.',
    expired: 'Visual challenge expired. Start a new verification.',
    exhausted: 'Visual attempts exhausted. Start a new login attempt.',
    cancelled: 'Cancelled', failed: 'Verification failed. Please try again.',
    activate: 'Verify this request',
  },
  ru: {
    instruction: 'Совместите фрагмент с вырезом', drag: 'Потяните вправо', position: 'Положение фрагмента',
    loading: 'Загружаем пазл…', ready: 'Отпустите или Enter',
    confirm: 'Проверить',
    checking: 'Проверяем пазл…', completed: 'Проверка завершена',
    retry: 'Не совпало. Попробуйте новый пазл.',
    expired: 'Время задания истекло. Начните проверку заново.',
    exhausted: 'Попытки закончились. Начните вход заново.',
    cancelled: 'Отменено', failed: 'Не удалось выполнить проверку. Попробуйте ещё раз.',
    activate: 'Подтвердить запрос',
  },
};
/** Explicit application locale, then element/page language, browser preferences, English. */
export function resolveLocale(element, locale) {
  const preferred = locale || element?.closest?.('[lang]')?.getAttribute('lang')
    || globalThis.document?.documentElement?.lang;
  const candidates = preferred ? [preferred] :
    [...(globalThis.navigator?.languages || []), globalThis.navigator?.language];
  for (const candidate of candidates) {
    const language = typeof candidate === 'string' ? candidate.trim().toLowerCase().split(/[-_]/)[0] : '';
    if (Object.hasOwn(visualMessages, language)) return language;
  }
  return 'en';
}
/** Render a busy indicator; call the returned cleanup when the operation settles. */
export function showLoading(element, message = 'Checking request...') {
  const status = document.createElement('span');
  status.className = 'gateveriq-loading';
  status.setAttribute('role', 'status');
  status.style.display = 'inline-flex'; status.style.alignItems = 'center'; status.style.gap = '8px';
  const spinner = document.createElement('span');
  spinner.className = 'gateveriq-spinner'; spinner.setAttribute('aria-hidden', 'true');
  Object.assign(spinner.style, {display: 'inline-block', width: '18px', height: '18px',
    boxSizing: 'border-box', flexShrink: '0', border: '2px solid currentColor',
    borderRightColor: 'transparent', borderRadius: '50%'});
  const label = document.createElement('span'); label.textContent = message;
  status.append(spinner); status.append(label);
  element.replaceChildren(status); element.setAttribute('aria-busy', 'true');
  const reducedMotion = globalThis.matchMedia?.('(prefers-reduced-motion: reduce)').matches;
  const animation = reducedMotion ? null : spinner.animate?.(
    [{transform: 'rotate(0deg)'}, {transform: 'rotate(360deg)'}],
    {duration: 800, iterations: Infinity});
  return () => { animation?.cancel(); element.setAttribute('aria-busy', 'false'); status.remove(); };
}

/** Collect only aggregate interactions inside the widget; never keys or coordinates. */
export function collectBehavior(element, challenge, signal, locale) {
  return new Promise((resolve, reject) => {
    const started = performance.now();
    const data = {challenge_id: challenge.challenge_id, elapsed_ms: 0, activation_ms: 0,
      trusted_activations: 0, pointer_events: 0, keyboard_events: 0, focus_changes: 0};
    const label = document.createElement('label');
    const checkbox = document.createElement('input');
    checkbox.type = 'checkbox';
    label.append(checkbox, ` ${visualMessages[resolveLocale(element, locale)].activate}`);
    element.replaceChildren(label);
    const count = field => event => {
      if (event.isTrusted) data[field] = Math.min(4096, data[field] + 1);
    };
    const pointer = count('pointer_events'), keyboard = count('keyboard_events'), focus = count('focus_changes');
    element.addEventListener('pointermove', pointer);
    element.addEventListener('pointerdown', pointer);
    element.addEventListener('keydown', keyboard);
    element.addEventListener('focusin', focus);
    const cleanup = () => {
      clearTimeout(timer);
      element.removeEventListener('pointermove', pointer);
      element.removeEventListener('pointerdown', pointer);
      element.removeEventListener('keydown', keyboard);
      element.removeEventListener('focusin', focus);
      checkbox.removeEventListener('change', activate);
      signal.removeEventListener('abort', abort);
      label.remove();
    };
    const abort = () => { cleanup(); reject(new Error('Cancelled')); };
    const activate = event => {
      const elapsed = Math.floor(performance.now() - started);
      if (!checkbox.checked) return;
      data.activation_ms = elapsed; data.elapsed_ms = elapsed;
      data.trusted_activations = event.isTrusted ? 1 : 0;
      cleanup(); resolve(data);
    };
    checkbox.addEventListener('change', activate);
    const remaining = Math.max(0, Math.min(600000, challenge.expires_at * 1000 - Date.now()));
    const timer = setTimeout(() => { cleanup(); reject(new Error('Challenge expired')); }, remaining);
    signal.addEventListener('abort', abort, {once: true});
    if (signal.aborted) abort();
  });
}
/** One verification at a time per widget. Returns an opaque token to the host app. */
export class GateveriqWidget {
  constructor(element) { this.element=element; this.running=false; }
  cancel() { this.controller?.abort(); this.worker?.terminate(); this.reject?.(new Error('Cancelled')); this.reject=null; }
  async verify(challenge, {solutionURL, signal, locale}={}) {
    if(this.running) throw Error('Verification already running');
    this.running=true; this.controller=new AbortController();
    let stopLoading;
    const cancel=()=>this.cancel(); signal?.addEventListener('abort',cancel,{once:true});
    this.element.setAttribute('role','status'); this.element.setAttribute('aria-live','polite');
    this.element.textContent='Checking request…';
    try {
      if(signal?.aborted) throw Error('Cancelled');
      const behavior = await collectBehavior(this.element, challenge, this.controller.signal, locale);
      stopLoading = showLoading(this.element);
      const nonce=await new Promise((resolve,reject)=>{
        this.reject=reject; this.worker=new Worker(workerURL);
        this.worker.onmessage=({data})=>data.error?reject(Error(data.error)):resolve(data.nonce);
        this.worker.onerror=()=>reject(Error('Worker failed'));
        this.worker.postMessage(challenge);
      });
      this.reject=null; this.worker.terminate();
      const response=await fetch(solutionURL,{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({challenge_id:challenge.challenge_id,nonce,behavior,browser:{webdriver:navigator.webdriver===true,cookies_enabled:navigator.cookieEnabled,languages_count:Math.min(navigator.languages.length,255)}}),signal:this.controller.signal});
      const result=await response.json();
      if (result.decision === 'additional_verification_required') {
        const error = new Error('Additional verification required');
        error.decision = result.decision;
        error.visual = result.visual; error.challenge = result.challenge;
        throw error;
      }
      if(!response.ok) throw Error(result.error?.code || 'Verification failed');
      if (typeof result.verification_token !== 'string' || !result.verification_token) throw Error('Missing verification token');
      this.element.textContent='Request checked';return result.verification_token;
    } catch(error){this.element.textContent='Check failed. Please try again.';throw error;}
    finally{stopLoading?.();this.worker?.terminate();this.reject=null;this.running=false;signal?.removeEventListener('abort',cancel);}
  }
}

/** Use an attempt prepared by your backend, never an integration secret. */
export async function verifyWebsiteAttempt(element, attempt, {serviceURL, signal = new AbortController().signal, locale, theme = 'auto', styleNonce} = {}) {
  locale = resolveLocale(element, locale);
  if (attempt.decision === 'additional_verification_required') {
    if (attempt.visual && attempt.challenge) {
      return verifyVisual(element, attempt.challenge, attempt.visual, {serviceURL, signal, locale, theme, styleNonce});
    }
    return {decision: attempt.decision};
  }
  if (attempt.decision !== 'verify' || !attempt.challenge) throw Error('Invalid website attempt');
  const solutionURL = new URL('/public/v1/web/solutions', serviceURL).href;
  const widget = new GateveriqWidget(element);
  try {
    const verification_token = await widget.verify(attempt.challenge, {solutionURL, signal, locale});
    return {decision: 'verified', verification_token};
  } catch (error) {
    if (error.decision === 'additional_verification_required') {
      if (error.visual) return verifyVisual(element, error.challenge || attempt.challenge, error.visual, {serviceURL, signal, locale, theme, styleNonce});
      return {decision: error.decision};
    }
    throw error;
  }
}

/** Compact slider, native range semantics, pointer/touch and keyboard confirmation. */
export function selectSlider(element, visual, signal, message = '', {locale, theme = 'auto', styleNonce} = {}) {
  const language = resolveLocale(element, locale), t = visualMessages[language];
  if (visual.kind !== 'slider_puzzle' || visual.width !== 280 || visual.height !== 140
      || visual.piece_size !== 40 || visual.max_position !== 240
      || !Number.isInteger(visual.piece_y) || visual.piece_y < 0 || visual.piece_y > 100
      || ![visual.background, visual.piece].every(src => typeof src === 'string' && src.startsWith('data:image/png;base64,'))) {
    return Promise.reject(Error('Invalid slider task'));
  }
  return new Promise((resolve, reject) => {
    const root = sliderRoot(element, theme, styleNonce);
    const card = document.createElement('section'); card.className = 'card'; card.lang = language;
    const heading = document.createElement('p'); heading.className = 'heading';
    heading.id = 'instruction'; heading.textContent = t.instruction;
    const stage = document.createElement('div'); stage.className = 'stage';
    const background = document.createElement('img'); background.className = 'background';
    background.alt = ''; background.draggable = false;
    const piece = document.createElement('img'); piece.className = 'piece';
    piece.alt = ''; piece.draggable = false;
    piece.style.width = `${visual.piece_size / visual.width * 100}%`;
    piece.style.top = `${visual.piece_y / visual.height * 100}%`;
    piece.style.left = '0%';
    stage.append(background, piece);
    const track = document.createElement('div'); track.className = 'track';
    const hint = document.createElement('span'); hint.className = 'track-label'; hint.textContent = t.drag;
    const handle = document.createElement('span'); handle.className = 'handle'; handle.textContent = '→';
    handle.style.left = '2px'; handle.setAttribute('aria-hidden','true');
    const range = document.createElement('input'); range.type = 'range';
    range.min = '0'; range.max = String(visual.max_position); range.step = '1'; range.value = '0';
    range.setAttribute('aria-label', t.position); range.setAttribute('aria-describedby','instruction help');
    range.disabled = true;
    track.append(hint, handle, range);
    const footer = document.createElement('div'); footer.className = 'footer';
    const note = document.createElement('p'); note.className = 'note'; note.id = 'help';
    note.setAttribute('role','status'); note.setAttribute('aria-live','polite'); note.textContent = t.loading;
    const confirm = document.createElement('button'); confirm.type = 'button';
    confirm.textContent = t.confirm; confirm.disabled = true;
    footer.append(note, confirm); card.append(heading, stage, track, footer); root.append(card);
    let loaded = 0, pointer = false, settled = false;
    const listeners = [];
    const listen = (target, event, callback) => { target.addEventListener(event, callback); listeners.push([target,event,callback]); };
    const cleanup = () => {
      clearTimeout(timer);
      for (const [target,event,callback] of listeners) target.removeEventListener(event,callback);
    };
    const fail = error => { if (settled) return; settled = true; cleanup(); range.disabled = confirm.disabled = true; reject(error); };
    const submit = () => {
      const position = Number(range.value);
      if (settled || loaded !== 2 || !Number.isInteger(position) || position <= 0 || position > visual.max_position) return;
      if (Date.now() >= visual.expires_at * 1000) return fail(Error(t.expired));
      settled = true; cleanup(); range.disabled = confirm.disabled = true; resolve(position);
    };
    listen(range,'input',() => {
      const position = Math.max(0,Math.min(visual.max_position,Number(range.value) || 0));
      piece.style.left = `${position / visual.width * 100}%`;
      handle.style.left = `calc(${position / visual.max_position * 100}% - ${position / visual.max_position * 38 - 2}px)`;
      hint.style.opacity = position > 0 ? '0' : '1';
      confirm.disabled = loaded !== 2 || position === 0;
    });
    listen(range,'pointerdown',() => { pointer = true; });
    listen(range,'pointercancel',() => { pointer = false; });
    listen(range,'change',() => { if (pointer) submit(); pointer = false; });
    listen(range,'keydown',event => {
      if (event.key === 'Enter' || event.key === ' ') { event.preventDefault(); submit(); }
    });
    listen(confirm,'click',submit);
    listen(signal,'abort',() => fail(Error(t.cancelled)));
    const timer = setTimeout(() => fail(Error(t.expired)), Math.max(0, Math.min(120000,visual.expires_at * 1000 - Date.now())));
    for (const [img,src] of [[background,visual.background],[piece,visual.piece]]) {
      listen(img,'load',() => {
        loaded++;
        if (loaded === 2 && !settled) { range.disabled = false; note.textContent = message || t.ready; }
      });
      listen(img,'error',() => fail(Error(t.failed)));
      img.src = src;
    }
    if (signal.aborted) fail(Error(t.cancelled));
  });
}
async function visualProof(challenge, signal) {
  if (signal.aborted) throw Error('Cancelled');
  const worker = new Worker(workerURL);
  let abort;
  try {
    return await new Promise((resolve, reject) => {
      abort = () => reject(new Error('Cancelled'));
      signal.addEventListener('abort', abort, {once: true});
      worker.onmessage = ({data}) => data.error ? reject(Error(data.error)) : resolve(data.nonce);
      worker.onerror = () => reject(Error('Worker failed'));
      worker.postMessage(challenge);
    });
  } finally { signal.removeEventListener('abort', abort); worker.terminate(); }
}
export async function verifyVisual(element, challenge, visual, {serviceURL, signal = new AbortController().signal, locale, theme = 'auto', styleNonce} = {}) {
  locale = resolveLocale(element, locale);
  const t = visualMessages[locale];
  const endpoint = new URL('/public/v1/web/visual/solutions', serviceURL);
  try {
    let nonce, message = '';
    const initialBudget = visual.max_attempts;
    for (let attempt = 0; attempt < initialBudget; attempt++) {
      const position = await selectSlider(element, visual, signal, message, {locale, theme, styleNonce});
      sliderState(element, t.checking);
      nonce ??= await visualProof(challenge, signal);
      const response = await fetch(endpoint, {method:'POST', headers:{'Content-Type':'application/json'},
        body:JSON.stringify({challenge_id:challenge.challenge_id, task_id:visual.task_id, nonce, position}), signal});
      const result = await response.json();
      if (response.ok && typeof result.verification_token === 'string' && result.verification_token) {
        sliderState(element, t.completed, 'success');
        return {decision:'verified', verification_token:result.verification_token};
      }
      if (result.error?.code !== 'INVALID_VISUAL_SOLUTION') throw Error(result.error?.code || 'Visual verification failed');
      if (!result.visual) throw Error('Missing replacement visual task');
      visual = result.visual;
      message = t.retry;

    }
    throw Error(t.exhausted);
  } catch (error) {
    const code = error.message;
    const message = signal.aborted ? t.cancelled
      : ['VISUAL_EXPIRED', 'CHALLENGE_EXPIRED'].includes(code) ? t.expired
      : code === 'VISUAL_ATTEMPTS_EXHAUSTED' ? t.exhausted
      : Object.values(t).includes(code) ? code : t.failed;
    sliderState(element, message, 'error');
    throw error;
  }
}
