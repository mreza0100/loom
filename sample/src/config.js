export const DATABASE_URL = process.env.DATABASE_URL || 'sqlite://store.db';
export const PORT = parseInt(process.env.PORT || '3000', 10);
export const JWT_SECRET = process.env.JWT_SECRET || 'dev-secret';
export const SESSION_TIMEOUT_MS = 30 * 60 * 1000;
export const MAX_CART_ITEMS = 50;
export const PAYMENT_RETRY_LIMIT = 3;
export const LOG_LEVEL = process.env.LOG_LEVEL || 'info';
