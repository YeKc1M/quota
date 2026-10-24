"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.reauthenticationMessage = reauthenticationMessage;
exports.isReauthenticationRequired = isReauthenticationRequired;
exports.tokenRefreshErrorMessage = tokenRefreshErrorMessage;
const REAUTHENTICATION_MESSAGES = {
    Codex: 'Codex authorization expired. Reauthenticate to continue.',
    'Claude Code': 'Claude Code authorization expired. Reauthenticate to continue.',
    Grok: 'Grok authorization is no longer valid. Reconnect Grok to continue.',
    'Kimi Code': 'Kimi Code rejected the stored token. Run Kimi Code to refresh it, or use /login.',
};
// Kimi Code keeps its own OAuth session on disk, so an expired local session
// is also a reauthentication case even though no token exchange happens here.
const KIMI_EXPIRED_CREDENTIALS_MESSAGE = "Kimi Code's stored token has expired. Run Kimi Code to refresh it, or use /login.";
function bodySignalsRejectedRefreshToken(body) {
    const normalized = body.toLowerCase();
    return [
        'invalid_grant',
        'refresh token expired',
        'refresh_token_expired',
        'refresh token revoked',
        'refresh_token_revoked',
    ].some((signal) => normalized.includes(signal));
}
function reauthenticationMessage(provider) {
    return REAUTHENTICATION_MESSAGES[provider];
}
function isReauthenticationRequired(message) {
    return (message != null
        && [...Object.values(REAUTHENTICATION_MESSAGES), KIMI_EXPIRED_CREDENTIALS_MESSAGE].includes(message));
}
function tokenRefreshErrorMessage(provider, status, body) {
    const isRejectedCredential = status === 401
        || status === 403
        || (status === 400 && bodySignalsRejectedRefreshToken(body));
    if (isRejectedCredential)
        return reauthenticationMessage(provider);
    return `${provider} token refresh returned ${status} with body length ${body.length}.`;
}
//# sourceMappingURL=authError.js.map