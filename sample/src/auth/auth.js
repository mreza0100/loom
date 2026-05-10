import { getUserByEmail, verifyPassword } from '../models/user.js';
import { createSession, destroySession } from './session.js';
import { AuthenticationError } from '../errors.js';
import { createLogger } from '../utils/logger.js';

const log = createLogger('auth');

export function login(email, password) {
  const user = getUserByEmail(email);
  if (!user) {
    log.warn(`Login attempt for unknown email: ${email}`);
    throw new AuthenticationError();
  }

  if (!verifyPassword(password, user.passwordHash)) {
    log.warn(`Failed login for user: ${user.name}`);
    throw new AuthenticationError();
  }

  const session = createSession(user.id);
  log.info(`User logged in: ${user.name}`);
  return { user: { id: user.id, name: user.name, role: user.role }, token: session.token };
}

export function logout(token) {
  destroySession(token);
  log.info('User logged out');
}

export function getCurrentUser(session) {
  if (!session) throw new AuthenticationError('No active session');
  return { userId: session.userId };
}
