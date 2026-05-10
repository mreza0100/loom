import { ValidationError } from '../errors.js';

export function validateEmail(email) {
  const re = /^[^\s@]+@[^\s@]+\.[^\s@]+$/;
  if (!re.test(email)) throw new ValidationError('email', 'Invalid format');
  return email.toLowerCase().trim();
}

const CURRENCY_DECIMALS = { USD: 2, EUR: 2, JPY: 0, BTC: 8 };

export function validatePrice(price, currency = 'USD') {
  const num = Number(price);
  if (isNaN(num) || num < 0) throw new ValidationError('price', 'Must be a non-negative number');
  const decimals = CURRENCY_DECIMALS[currency];
  if (decimals === undefined) throw new ValidationError('currency', `Unsupported currency: ${currency}`);
  const factor = Math.pow(10, decimals);
  return Math.round(num * factor) / factor;
}

export function validateQuantity(quantity) {
  const num = parseInt(quantity, 10);
  if (isNaN(num) || num < 1) throw new ValidationError('quantity', 'Must be a positive integer');
  return num;
}

export function validateRequired(fields, data) {
  for (const field of fields) {
    if (data[field] === undefined || data[field] === null || data[field] === '') {
      throw new ValidationError(field, 'Required field is missing');
    }
  }
}

export function sanitizeString(input) {
  if (typeof input !== 'string') return '';
  return input.trim().replace(/[<>]/g, '');
}
