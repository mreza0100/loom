import { findById, findAll, insert, update } from '../database.js';
import { validateEmail, validateRequired } from '../utils/validator.js';
import { NotFoundError } from '../errors.js';
import { createLogger } from '../utils/logger.js';

const log = createLogger('user-model');
const COLLECTION = 'users';

export function createUser(data) {
  validateRequired(['name', 'email', 'password'], data);
  const email = validateEmail(data.email);

  const existing = findAll(COLLECTION).find(u => u.email === email);
  if (existing) throw new Error(`User with email ${email} already exists`);

  return insert(COLLECTION, {
    name: data.name,
    email,
    passwordHash: hashPassword(data.password),
    role: data.role || 'customer',
  });
}

export function getUserById(id) {
  const user = findById(COLLECTION, id);
  if (!user) throw new NotFoundError('User', id);
  return user;
}

export function getUserByEmail(email) {
  return findAll(COLLECTION).find(u => u.email === email) || null;
}

export function updateUser(id, changes) {
  const user = getUserById(id);
  if (changes.email) changes.email = validateEmail(changes.email);
  log.info(`Updating user ${user.name}`, { fields: Object.keys(changes) });
  return update(COLLECTION, id, changes);
}

export function listUsers() {
  return findAll(COLLECTION);
}

function hashPassword(password) {
  return Buffer.from(password).toString('base64');
}

export function verifyPassword(password, hash) {
  return hashPassword(password) === hash;
}
