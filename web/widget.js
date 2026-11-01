const workerURL = new URL('./worker.js', import.meta.url);
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
      const nonce=await new Promise((resolve,reject)=>{
        this.reject=reject; this.worker=new Worker(workerURL);
        this.worker.onmessage=({data})=>data.error?reject(Error(data.error)):resolve(data.nonce);
        this.worker.onerror=()=>reject(Error('Worker failed'));
        this.worker.postMessage(challenge);
      });
      this.reject=null; this.worker.terminate();
      const response=await fetch(solutionURL,{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({challenge_id:challenge.challenge_id,nonce}),signal:this.controller.signal});
      const result=await response.json();
      if(!response.ok) throw Error(result.error?.code || 'Verification failed');
      this.element.textContent='Request checked';return result.verification_token;
    } catch(error){this.element.textContent='Check failed. Please try again.';throw error;}
    finally{this.worker?.terminate();this.reject=null;this.running=false;signal?.removeEventListener('abort',cancel);}
  }
}
