// Protocol v1: SHA256(UTF8(domain) || raw ID || raw salt || eight-byte nonce).
const decode = s => Uint8Array.from(atob(s.replace(/-/g,'+').replace(/_/g,'/')), x=>x.charCodeAt(0));
const encode = b => btoa(String.fromCharCode(...b)).replace(/\+/g,'-').replace(/\//g,'_').replace(/=+$/,'');
self.onmessage = async ({data:c}) => {
  try {
    if(c.algorithm !== 'sha256-pow-v1' || !Number.isInteger(c.difficulty_bits) || c.difficulty_bits<1 || c.difficulty_bits>24 || decode(c.challenge_id).length!==32 || decode(c.salt).length!==32) throw Error('Invalid challenge');
    const prefix=new TextEncoder().encode('gateveriq:pow:v1');
    const bytes=new Uint8Array(prefix.length+72);
    bytes.set(prefix); bytes.set(decode(c.challenge_id),prefix.length);bytes.set(decode(c.salt),prefix.length+32);
    const view=new DataView(bytes.buffer); const offset=prefix.length+64;
    for(let n=0n;n<2n**64n;n++) {
      if(n%256n===0n && Date.now()>=c.expires_at*1000) throw Error('Challenge expired');
      view.setBigUint64(offset,n,true);
      const hash=new Uint8Array(await crypto.subtle.digest('SHA-256',bytes));
      let valid=true;for(let bit=0;bit<c.difficulty_bits;bit++) if(hash[bit>>3] & (128>>(bit%8))){valid=false;break;}
      if(valid){postMessage({nonce:encode(bytes.slice(offset))});return;}
    }
    throw Error('Nonce space exhausted');
  } catch(e){postMessage({error:e.message});}
};
