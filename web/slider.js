// Shadow DOM keeps host application styles from changing the slider layout.
// Inherited --gateveriq-* variables remain available for application themes.
export const sliderCSS = `
:host{--gv-background:var(--gateveriq-background,#fff);--gv-text:var(--gateveriq-text,#253342);--gv-muted:var(--gateveriq-muted,#6c7a89);--gv-border:var(--gateveriq-border,#e2e7ed);--gv-surface:var(--gateveriq-surface,#f2f5f8);--gv-accent:var(--gateveriq-accent,#3569e8);--gv-radius:var(--gateveriq-radius,14px);display:block;width:304px;max-width:100%;color-scheme:light;font:12px/1.4 ui-sans-serif,system-ui,-apple-system,BlinkMacSystemFont,"Segoe UI",sans-serif}
:host([data-theme=dark]){--gv-background:var(--gateveriq-background,#19212e);--gv-text:var(--gateveriq-text,#e6edf7);--gv-muted:var(--gateveriq-muted,#9baac0);--gv-border:var(--gateveriq-border,#334155);--gv-surface:var(--gateveriq-surface,#242f40);--gv-accent:var(--gateveriq-accent,#85acff);color-scheme:dark}
@media(prefers-color-scheme:dark){:host([data-theme=auto]){--gv-background:var(--gateveriq-background,#19212e);--gv-text:var(--gateveriq-text,#e6edf7);--gv-muted:var(--gateveriq-muted,#9baac0);--gv-border:var(--gateveriq-border,#334155);--gv-surface:var(--gateveriq-surface,#242f40);--gv-accent:var(--gateveriq-accent,#85acff);color-scheme:dark}}
*{box-sizing:border-box}
.card{padding:11px;border:1px solid var(--gv-border);border-radius:var(--gv-radius);background:var(--gv-background);color:var(--gv-text)}
.heading{margin:0 0 9px;font-size:12px;font-weight:600;letter-spacing:.01em}
.stage{position:relative;width:100%;aspect-ratio:2;overflow:hidden;border-radius:8px;background:var(--gv-surface)}
.background{display:block;width:100%;height:100%;user-select:none;-webkit-user-drag:none}
.piece{position:absolute;pointer-events:none;user-select:none;-webkit-user-drag:none;filter:drop-shadow(0 1px 2px #0008)}
.track{direction:ltr;position:relative;height:40px;margin-top:10px;border:1px solid var(--gv-border);border-radius:8px;background:var(--gv-surface);overflow:hidden}
.track-label{position:absolute;inset:0;display:grid;place-items:center;color:var(--gv-muted);font-size:11px;pointer-events:none}
.handle{position:absolute;inset-block:2px;width:34px;display:grid;place-items:center;border-radius:5px;background:var(--gv-background);border:1px solid var(--gv-border);color:var(--gv-accent);font-size:20px;box-shadow:0 1px 3px #0001;pointer-events:none}
input{position:absolute;inset:0;width:100%;height:100%;margin:0;appearance:none;-webkit-appearance:none;background:transparent;cursor:grab;touch-action:pan-y;opacity:0}
input:active{cursor:grabbing}
.track:focus-within{outline:2px solid var(--gv-accent);outline-offset:2px}
input::-webkit-slider-thumb{appearance:none;-webkit-appearance:none;width:38px;height:38px}
input::-moz-range-thumb{width:38px;height:38px;border:0;background:transparent}
.footer{display:flex;align-items:center;gap:8px;justify-content:space-between;margin-top:9px;min-height:28px}
.note{margin:0;color:var(--gv-muted);font-size:11px;flex:1;min-width:0}
button{font:inherit;font-size:11px;font-weight:600;border:0;border-radius:6px;background:var(--gv-accent);color:var(--gv-background);padding:6px 10px;cursor:pointer;flex-shrink:0}
button:focus-visible{outline:2px solid var(--gv-accent);outline-offset:3px}
button:disabled{opacity:.45;cursor:default}
.card[data-state=error] .note{color:var(--gv-text)}
.card[data-state=success]{display:flex;align-items:center;gap:8px;min-height:52px}
.success-icon{font-size:18px;color:var(--gv-accent)}
@media(forced-colors:active){.handle,.track,button{border:1px solid ButtonText}.handle{color:ButtonText}button{background:ButtonFace;color:ButtonText}}
`;

export function sliderRoot(element, theme = 'auto', styleNonce) {
  const host = document.createElement('div');
  host.className = 'gateveriq-slider';
  const preset = typeof theme === 'string' ? theme : theme?.preset || 'auto';
  host.setAttribute('data-theme', ['light', 'dark', 'auto'].includes(preset) ? preset : 'auto');
  const properties = {background:'background', text:'text', muted:'muted', border:'border',
    surface:'surface', accent:'accent', radius:'radius'};
  if (theme && typeof theme === 'object') {
    for (const [key, property] of Object.entries(properties)) {
      const value = theme[key];
      const cssProperty = key === 'radius' ? 'border-radius' : 'color';
      if (typeof value === 'string' && globalThis.CSS?.supports(cssProperty, value)) {
        host.style.setProperty(`--gateveriq-${property}`, value);
      }
    }
  }
  const root = host.attachShadow({mode:'open'});
  const style = document.createElement('style');
  if (styleNonce) style.nonce = styleNonce;
  style.textContent = sliderCSS;
  root.append(style);
  element.replaceChildren(host);
  return root;
}

export function sliderState(element, message, state = 'busy') {
  const root = element.querySelector('.gateveriq-slider')?.shadowRoot;
  if (!root) return;
  const card = root.querySelector('.card');
  card?.setAttribute('data-state', state);
  card?.setAttribute('aria-busy', String(state === 'busy'));
  const note = root.querySelector('.note');
  if (note) note.textContent = message;
  for (const control of root.querySelectorAll('input,button')) control.disabled = true;
  if (state === 'success') {
    const icon = document.createElement('span'); icon.className = 'success-icon';
    icon.textContent = '✓'; icon.setAttribute('aria-hidden','true');
    const label = document.createElement('span'); label.textContent = message;
    card.replaceChildren(icon,label);
  }
}
