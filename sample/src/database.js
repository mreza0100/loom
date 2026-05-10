import { DATABASE_URL } from './config.js';
import { createLogger } from './utils/logger.js';

const log = createLogger('database');

const store = {
  users: new Map(),
  products: new Map(),
  orders: new Map(),
  sessions: new Map(),
};

let idCounter = 1000;

export function generateId() {
  return `id_${++idCounter}`;
}

export function getCollection(name) {
  if (!store[name]) {
    log.warn(`Creating new collection: ${name}`);
    store[name] = new Map();
  }
  return store[name];
}

export function findById(collection, id) {
  return getCollection(collection).get(id) || null;
}

export function findAll(collection) {
  return Array.from(getCollection(collection).values());
}

export function insert(collection, record) {
  const id = record.id || generateId();
  const entry = { ...record, id, createdAt: new Date().toISOString() };
  getCollection(collection).set(id, entry);
  log.info(`Inserted into ${collection}`, { id });
  return entry;
}

export function update(collection, id, changes) {
  const existing = findById(collection, id);
  if (!existing) return null;
  const updated = { ...existing, ...changes, updatedAt: new Date().toISOString() };
  getCollection(collection).set(id, updated);
  log.info(`Updated ${collection}`, { id });
  return updated;
}

export function remove(collection, id) {
  const deleted = getCollection(collection).delete(id);
  if (deleted) log.info(`Removed from ${collection}`, { id });
  return deleted;
}

export function initDatabase() {
  log.info(`Database initialized with URL: ${DATABASE_URL}`);
  return store;
}
