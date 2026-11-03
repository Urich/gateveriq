const workerURL = new URL('./worker.js', import.meta.url);
/** Collect only aggregate interactions inside the widget; never keys or coordinates. */
export function collectBehavior(element, challenge, signal) {
  return new Promise((resolve, reject) => {
    const started = performance.now();
    const data = {challenge_id: challenge.challenge_id, elapsed_ms: 0, activation_ms: 0,
      trusted_activations: 0, pointer_events: 0, keyboard_events: 0, focus_changes: 0};
    const button = document.createElement('button');
    button.type = 'button'; button.textContent = 'Confirm verification';
    element.replaceChildren(button);
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
      button.removeEventListener('click', activate);
      signal.removeEventListener('abort', abort);
      button.remove();
    };
    const abort = () => { cleanup(); reject(new Error('Cancelled')); };
    const activate = event => {
      const elapsed = Math.floor(performance.now() - started);
      // Native button activation supports mouse, touch, keyboard and assistive tools.
      if (!event.isTrusted || elapsed < 300) return;
      data.activation_ms = elapsed; data.elapsed_ms = elapsed; data.trusted_activations = 1;
      cleanup(); resolve(data);
    };
    button.addEventListener('click', activate);
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
  async verify(challenge, {solutionURL, signal}={}) {
    if(this.running) throw Error('Verification already running');
    this.running=true; this.controller=new AbortController();
    const cancel=()=>this.cancel(); signal?.addEventListener('abort',cancel,{once:true});
    this.element.setAttribute('role','status'); this.element.setAttribute('aria-live','polite');
    this.element.textContent='Checking request…';
    try {
      if(signal?.aborted) throw Error('Cancelled');
      const behavior = await collectBehavior(this.element, challenge, this.controller.signal);
      this.element.textContent = 'Checking request...';
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
    finally{this.worker?.terminate();this.reject=null;this.running=false;signal?.removeEventListener('abort',cancel);}
  }
}

/** Use an attempt prepared by your backend, never an integration secret. */
export async function verifyWebsiteAttempt(element, attempt, {serviceURL, signal} = {}) {
  if (attempt.decision === 'additional_verification_required') {
    if (attempt.visual && attempt.challenge) return verifyVisual(element, attempt.challenge, attempt.visual, {serviceURL, signal});
    return {decision: attempt.decision};
  }
  if (attempt.decision !== 'verify' || !attempt.challenge) throw Error('Invalid website attempt');
  const solutionURL = new URL('/public/v1/web/solutions', serviceURL).href;
  const widget = new GateveriqWidget(element);
  try {
    const verification_token = await widget.verify(attempt.challenge, {solutionURL, signal});
    return {decision: 'verified', verification_token};
  } catch (error) {
    if (error.decision === 'additional_verification_required') {
      if (error.visual) return verifyVisual(element, error.challenge || attempt.challenge, error.visual, {serviceURL, signal});
      return {decision: error.decision};
    }
    throw error;
  }
}

/** Image contents have neutral labels; answers are never encoded in DOM metadata. */
export function selectImages(element, visual, signal, message = '') {
  return new Promise((resolve, reject) => {
    const selected = new Set();
    const heading = document.createElement('p'); heading.textContent = visual.instruction;
    const reference = document.createElement('img'); reference.src = visual.reference;
    reference.alt = 'Reference illustration'; reference.width = 112; reference.height = 112;
    reference.style.borderRadius = '14px'; reference.style.objectFit = 'contain';
    const grid = document.createElement('div');
    grid.style.display = 'grid'; grid.style.gridTemplateColumns = 'repeat(4, minmax(0, 1fr))';
    grid.style.gap = '8px'; grid.style.maxWidth = '440px';
    const listeners = [];
    const confirm = document.createElement('button'); confirm.type = 'button';
    confirm.textContent = 'Confirm selection'; confirm.disabled = true;
    const note = document.createElement('p'); note.textContent = message;
    note.setAttribute('role', 'status');
    visual.images.forEach((src, index) => {
      const button = document.createElement('button'); button.type = 'button';
      button.setAttribute('aria-label', `Image ${index + 1}`);
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
    const cleanup = () => {
      clearTimeout(timer); signal.removeEventListener('abort', abort);
      confirm.removeEventListener('click', submit);
      for (const [button, listener] of listeners) button.removeEventListener('click', listener);
    };
    const abort = () => { cleanup(); reject(new Error('Cancelled')); };
    const submit = () => { if (!selected.size) return; cleanup(); resolve([...selected].sort((a,b) => a-b)); };
    confirm.addEventListener('click', submit);
    const timer = setTimeout(() => { cleanup(); reject(new Error('Visual challenge expired')); }, Math.max(0, visual.expires_at * 1000 - Date.now()));
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
export async function verifyVisual(element, challenge, visual, {serviceURL, signal = new AbortController().signal} = {}) {
  const endpoint = new URL('/public/v1/web/visual/solutions', serviceURL);
  try {
    let nonce, message = '';
    const initialBudget = visual.max_attempts;
    for (let attempt = 0; attempt < initialBudget; attempt++) {
      const selected = await selectImages(element, visual, signal, message);
      element.textContent = 'Checking images...';
      nonce ??= await visualProof(challenge, signal);
      const response = await fetch(endpoint, {method:'POST', headers:{'Content-Type':'application/json'},
        body:JSON.stringify({challenge_id:challenge.challenge_id, task_id:visual.task_id, nonce, selected}), signal});
      const result = await response.json();
      if (response.ok && typeof result.verification_token === 'string' && result.verification_token) {
        element.textContent = 'Verification completed';
        return {decision:'verified', verification_token:result.verification_token};
      }
      if (result.error?.code !== 'INVALID_VISUAL_SOLUTION') throw Error(result.error?.code || 'Visual verification failed');
      if (!result.visual) throw Error('Missing replacement visual task');
      visual = result.visual;
      message = 'Selection was incorrect. Try the new images.';
    }
    throw Error('Visual attempts exhausted. Start a new login attempt.');
  } catch (error) { element.textContent = error.message; throw error; }
}
