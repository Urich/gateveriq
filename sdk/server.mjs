import {randomBytes} from 'node:crypto';

/** Backend-only client. Never bundle this module or its secret into browser code. */
export class GateveriqClient {
  constructor({integrationId, secret, internalURL = 'http://127.0.0.1:9081', fetchImpl = globalThis.fetch}) {
    this.integrationId = integrationId;
    this.secret = secret;
    this.internalURL = internalURL;
    this.fetch = fetchImpl;
  }

  async post(path, body) {
    const response = await this.fetch(new URL(path, this.internalURL), {
      method: 'POST',
      headers: {'Content-Type': 'application/json', Authorization: `Bearer ${this.secret}`},
      body: JSON.stringify({integration_id: this.integrationId, ...body}),
      signal: AbortSignal.timeout(10000),
    });
    const result = await response.json();
    if (!response.ok) {
      const error = new Error(result.error?.code || 'Gateveriq request failed');
      error.status = response.status;
      error.code = result.error?.code;
      throw error;
    }
    return result;
  }

  /** Save contextId, requestId and the account/action in your server session. */
  createAttempt({accountId, origin, action = 'login'}) {
    return {accountId, origin, action, contextId: randomBytes(32).toString('base64url'),
      requestId: randomBytes(32).toString('base64url')};
  }

  async prepareAttempt({accountId, origin, action = 'login',
    contextId = randomBytes(32).toString('base64url'), requestId = randomBytes(32).toString('base64url')}) {
    const attempt = await this.post('/internal/v1/web/attempts', {
      action, context_id: contextId, account_id: accountId, origin,
    });
    return {contextId, requestId, action, attempt};
  }

  async prepareLogin(options) { return this.prepareAttempt(options); }

  async consume({contextId, requestId, action = 'login', verificationToken}) {
    const result = await this.post('/internal/v1/verifications/consume', {
      action, context_id: contextId, request_id: requestId, verification_token: verificationToken,
    });
    if (result.valid !== true) throw Error('Verification was not accepted');
    return result;
  }

  async reportOutcome({contextId, requestId, action = 'login', success}) {
    if (typeof success !== 'boolean') throw TypeError('success must be boolean');
    return this.post('/internal/v1/web/outcomes', {
      action, context_id: contextId, request_id: requestId, success,
    });
  }
}
