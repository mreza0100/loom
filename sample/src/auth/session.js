import { SESSION_TIMEOUT_MS } from '../config.js';
import { getCollection, generateId } from '../database.js';
import { createLogger } from '../utils/logger.js';

const log = createLogger('session');

export function createSession(userId) {
  const sessions = getCollection('sessions');
  const token = generateId() + '_' + Date.now().toString(36);
  const session = {
    token,
    userId,
    createdAt: Date.now(),
    expiresAt: Date.now() + SESSION_TIMEOUT_MS,
  };
  sessions.set(token, session);
  log.info(`Session created for user ${userId}`);
  return session;
}

export function resolveSession(token) {
  const sessions = getCollection('sessions');
  const session = sessions.get(token);
  if (!session) return null;
  if (Date.now() > session.expiresAt) {
    sessions.delete(token);
    log.info(`Session expired for user ${session.userId}`);
    return null;
  }
  return session;
}

export function refreshSession(token) {
  const session = resolveSession(token);
  if (!session) return null;
  session.expiresAt = Date.now() + SESSION_TIMEOUT_MS;
  getCollection('sessions').set(token, session);
  log.debug(`Session refreshed for user ${session.userId}`);
  return session;
}

export function destroySession(token) {
  const sessions = getCollection('sessions');
  const deleted = sessions.delete(token);
  if (deleted) log.info('Session destroyed');
  return deleted;
}

export function cleanExpiredSessions() {
  const sessions = getCollection('sessions');
  let cleaned = 0;
  for (const [token, session] of sessions) {
    if (Date.now() > session.expiresAt) {
      sessions.delete(token);
      cleaned++;
    }
  }
  if (cleaned > 0) log.info(`Cleaned ${cleaned} expired sessions`);
  return cleaned;
}
