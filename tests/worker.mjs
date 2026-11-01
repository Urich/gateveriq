import {readFileSync} from 'node:fs';
import {runInNewContext} from 'node:vm';
import {webcrypto,createHash} from 'node:crypto';
import assert from 'node:assert/strict';
const challenge={challenge_id:Buffer.alloc(32,0).toString('base64url'),salt:Buffer.alloc(32,1).toString('base64url'),algorithm:'sha256-pow-v1',difficulty_bits:8,expires_at:Math.floor(Date.now()/1000)+120};
const result=await new Promise((resolve,reject)=>{
 const scope={crypto:webcrypto,TextEncoder,Uint8Array,DataView,atob,btoa,Date,Error,Number,postMessage:resolve,self:{}};
 runInNewContext(readFileSync(new URL('../web/worker.js',import.meta.url),'utf8'),scope);
 scope.self.onmessage({data:challenge}).catch(reject);
});
assert.ok(!result.error,result.error);
const digest=createHash('sha256').update(Buffer.concat([Buffer.from('gateveriq:pow:v1'),Buffer.alloc(32),Buffer.alloc(32,1),Buffer.from(result.nonce,'base64url')])).digest();
assert.equal(digest[0],0);assert.equal(Buffer.from(result.nonce,'base64url').length,8);
console.log('Browser worker interoperability passed; nonce:',result.nonce);
