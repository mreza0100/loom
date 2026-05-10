import { resolveSession, refreshSession, destroySession } from './session.js';
import { getUserById } from '../models/user.js';
import { AuthenticationError, NotFoundError } from '../errors.js';
import { createLogger } from '../utils/logger.js';

const log = createLogger('auth-middleware');

export function authenticate(req) {
  const token = extractToken(req);
  if (!token) throw new AuthenticationError('Missing authorization token');

  const session = resolveSession(token);
  if (!session) throw new AuthenticationError('Invalid or expired session');

  refreshSession(token);

  let user;
  try {
    user = getUserById(session.userId);
  } catch (err) {
    if (err instanceof NotFoundError) {
      destroySession(token);
      log.warn(`Session references deleted user ${session.userId}, session destroyed`);
      throw new AuthenticationError('Account no longer exists');
    }
    throw err;
  }

  log.debug(`Authenticated: ${user.name}`);
  return { ...req, user, session };
}

export function requireRole(role) {
  return function checkRole(req) {
    if (!req.user) throw new AuthenticationError('Not authenticated');
    if (req.user.role !== role && req.user.role !== 'admin') {
      throw new AuthenticationError(`Requires role: ${role}`);
    }
    return req;
  };
}

function extractToken(req) {
  const header = req.headers?.authorization;
  if (!header) return null;
  const [scheme, token] = header.split(' ');
  if (scheme !== 'Bearer') return null;
  return token;
}
