import {GateveriqClient} from '../sdk/server.mjs';

// Backend-only teaching example. Replace Maps with durable transactional storage.
// Resolve owner from an authenticated session or a random operation secret.
export class RegistrationFlow {
  constructor(gate) { this.gate = gate; this.operations = new Map(); }

  // Save the operation BEFORE network I/O; resume it after network failures.
  create(owner, accountId, origin, action = 'register') {
    if (!['register', 'email_verify'].includes(action)) throw Error('Invalid action');
    const pending = this.gate.createAttempt({accountId, origin, action});
    this.operations.set(pending.requestId, {owner, pending});
    return pending.requestId;
  }

  get(owner, requestId) {
    const operation = this.operations.get(requestId);
    if (!operation || operation.owner !== owner) throw Error('Unknown operation');
    return operation;
  }

  async prepare(owner, requestId) {
    const op = this.get(owner, requestId);
    const result = await this.gate.prepareAttempt(op.pending);
    return result.attempt; // Only the browser payload; keep pending on the backend.
  }

  // payload is immutable for a requestId. A different code needs a NEW operation.
  complete(owner, requestId, token, payload) {
    const op = this.get(owner, requestId);
    const encoded = JSON.stringify(payload);
    if (op.payload !== undefined && op.payload !== encoded) throw Error('Request conflict');
    op.payload = encoded;
    if (!op.result) {
      op.result = (async () => {
        const receipt = await this.gate.consume({...op.pending, verificationToken:token});
        if (receipt.verification_method !== 'pow_visual') throw Error('Images required');
        // Fake business check. No email delivery or real credential handling.
        const success = op.pending.action === 'register' || payload.demoAnswer === 'accepted';
        await this.gate.reportOutcome({...op.pending, success});
        // A successful registration may grant ONE initial email-code submission.
        // Store and consume that grant atomically in your application database.
        return {success, initialEmailGrant: success && op.pending.action === 'register'};
      })();
      // Exact retries resume after transport errors; real operations require a DB transaction.
      op.result.catch(() => { op.result = undefined; });
    }
    return op.result;
  }
}

export function configuredFlow() {
  return new RegistrationFlow(new GateveriqClient({
    integrationId:process.env.GATEVERIQ_ID, secret:process.env.GATEVERIQ_SECRET,
  }));
}
