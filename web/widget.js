const workerURL = new URL('./worker.js', import.meta.url);
const visualMessages = {
  en: {
    instruction: 'Select all illustrations showing the same type of object as the reference. Colors and poses may differ.',
    reference: 'Reference illustration', image: 'Image', confirm: 'Confirm selection',
    checking: 'Checking images...', completed: 'Verification completed',
    retry: 'Selection was incorrect. Try the new images.',
    expired: 'Visual challenge expired. Start a new verification.',
    exhausted: 'Visual attempts exhausted. Start a new login attempt.',
    cancelled: 'Cancelled', failed: 'Verification failed. Please try again.',
    activate: 'Verify this request',
  },
  ru: {
    instruction: 'Выберите все картинки с таким же типом объекта, как на образце. Цвета и позы могут отличаться.',
    reference: 'Образец для сравнения', image: 'Картинка', confirm: 'Подтвердить выбор',
    checking: 'Проверяем картинки...', completed: 'Проверка завершена',
    retry: 'Выбор неверный. Попробуйте новые картинки.',
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
export async function verifyWebsiteAttempt(element, attempt, {serviceURL, signal = new AbortController().signal, locale} = {}) {
  locale = resolveLocale(element, locale);
  if (attempt.decision === 'additional_verification_required') {
    if (attempt.visual && attempt.challenge) {
      await collectBehavior(element, attempt.challenge, signal, locale);
      return verifyVisual(element, attempt.challenge, attempt.visual, {serviceURL, signal, locale});
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
      if (error.visual) return verifyVisual(element, error.challenge || attempt.challenge, error.visual, {serviceURL, signal, locale});
      return {decision: error.decision};
    }
    throw error;
  }
}

/** Image contents have neutral labels; answers are never encoded in DOM metadata. */
export function selectImages(element, visual, signal, message = '', {locale} = {}) {
  const language = resolveLocale(element, locale), t = visualMessages[language];
  return new Promise((resolve, reject) => {
    const selected = new Set();
    const heading = document.createElement('p');
    heading.textContent = visual.instruction_key === 'match_reference'
      || visual.instruction === visualMessages.en.instruction ? t.instruction : visual.instruction;
    const reference = document.createElement('img'); reference.src = visual.reference;
    reference.alt = t.reference; reference.width = 112; reference.height = 112;
    reference.style.borderRadius = '14px'; reference.style.objectFit = 'contain';
    const grid = document.createElement('div');
    grid.style.display = 'grid'; grid.style.gridTemplateColumns = 'repeat(4, minmax(0, 1fr))';
    grid.style.gap = '8px'; grid.style.maxWidth = '440px';
    const listeners = [];
    const confirm = document.createElement('button'); confirm.type = 'button';
    confirm.textContent = t.confirm; confirm.disabled = true;
    const note = document.createElement('p'); note.textContent = message;
    note.setAttribute('role', 'status');
    visual.images.forEach((src, index) => {
      const button = document.createElement('button'); button.type = 'button';
      button.setAttribute('aria-label', `${t.image} ${index + 1}`);
      button.setAttribute('aria-pressed', 'false');
      button.style.margin = '0'; button.style.minWidth = '0';
      button.style.padding = '0'; button.style.border = '3px solid transparent';
      button.style.borderRadius = '12px'; button.style.overflow = 'hidden';
      button.style.background = '#f7f5ef'; button.style.cursor = 'pointer';
      const img = document.createElement('img'); img.src = src; img.alt = '';
      img.style.width = '100%'; img.style.display = 'block'; button.append(img);
      const toggle = () => {
        if (selected.has(index)) selected.delete(index); else selected.add(index);
        button.setAttribute('aria-pressed', String(selected.has(index)));
        button.style.borderColor = selected.has(index) ? '#2563eb' : 'transparent';
        confirm.disabled = selected.size === 0;
      };
      button.addEventListener('click', toggle); listeners.push([button, toggle]); grid.append(button);
    });
    element.replaceChildren(heading, reference, grid, note, confirm);
    for (const child of [heading, reference, grid, note, confirm]) child.setAttribute('lang', language);
    const cleanup = () => {
      clearTimeout(timer); signal.removeEventListener('abort', abort);
      confirm.removeEventListener('click', submit);
      for (const [button, listener] of listeners) button.removeEventListener('click', listener);
    };
    const abort = () => { cleanup(); reject(new Error(t.cancelled)); };
    const submit = () => { if (!selected.size) return; cleanup(); resolve([...selected].sort((a,b) => a-b)); };
    confirm.addEventListener('click', submit);
    const timer = setTimeout(() => { cleanup(); reject(new Error(t.expired)); }, Math.max(0, visual.expires_at * 1000 - Date.now()));
    signal.addEventListener('abort', abort, {once: true});
    if (signal.aborted) abort();
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
export async function verifyVisual(element, challenge, visual, {serviceURL, signal = new AbortController().signal, locale} = {}) {
  locale = resolveLocale(element, locale);
  const t = visualMessages[locale];
  const endpoint = new URL('/public/v1/web/visual/solutions', serviceURL);
  let stopLoading;
  try {
    let nonce, message = '';
    const initialBudget = visual.max_attempts;
    for (let attempt = 0; attempt < initialBudget; attempt++) {
      const selected = await selectImages(element, visual, signal, message, {locale});
      stopLoading = showLoading(element, t.checking);
      nonce ??= await visualProof(challenge, signal);
      const response = await fetch(endpoint, {method:'POST', headers:{'Content-Type':'application/json'},
        body:JSON.stringify({challenge_id:challenge.challenge_id, task_id:visual.task_id, nonce, selected}), signal});
      const result = await response.json();
      if (response.ok && typeof result.verification_token === 'string' && result.verification_token) {
        element.textContent = t.completed;
        return {decision:'verified', verification_token:result.verification_token};
      }
      if (result.error?.code !== 'INVALID_VISUAL_SOLUTION') throw Error(result.error?.code || 'Visual verification failed');
      if (!result.visual) throw Error('Missing replacement visual task');
      visual = result.visual;
      message = t.retry;
      stopLoading(); stopLoading = null;
    }
    throw Error(t.exhausted);
  } catch (error) {
    const code = error.message;
    element.textContent = signal.aborted ? t.cancelled
      : ['VISUAL_EXPIRED', 'CHALLENGE_EXPIRED'].includes(code) ? t.expired
      : code === 'VISUAL_ATTEMPTS_EXHAUSTED' ? t.exhausted
      : Object.values(t).includes(code) ? code : t.failed;
    throw error;
  }
  finally { stopLoading?.(); }
}
