import { findById, findAll, insert, update, remove } from '../database.js';
import { validatePrice, validateRequired, sanitizeString } from '../utils/validator.js';
import { NotFoundError } from '../errors.js';
import { createLogger } from '../utils/logger.js';

const log = createLogger('product-model');
const COLLECTION = 'products';

export function createProduct(data) {
  validateRequired(['name', 'price'], data);
  const price = validatePrice(data.price);

  return insert(COLLECTION, {
    name: sanitizeString(data.name),
    description: sanitizeString(data.description || ''),
    price,
    stock: parseInt(data.stock || '0', 10),
    category: data.category || 'uncategorized',
  });
}

export function getProductById(id) {
  const product = findById(COLLECTION, id);
  if (!product) throw new NotFoundError('Product', id);
  return product;
}

export function listProducts(category) {
  const all = findAll(COLLECTION);
  if (category) return all.filter(p => p.category === category);
  return all;
}

export function updateStock(id, quantityChange) {
  const product = getProductById(id);
  const newStock = product.stock + quantityChange;
  if (newStock < 0) throw new Error(`Insufficient stock for product ${id}`);
  log.info(`Stock update for ${product.name}: ${product.stock} -> ${newStock}`);
  return update(COLLECTION, id, { stock: newStock });
}

export function updateProduct(id, changes) {
  getProductById(id);
  if (changes.price) changes.price = validatePrice(changes.price);
  if (changes.name) changes.name = sanitizeString(changes.name);
  return update(COLLECTION, id, changes);
}

export function deleteProduct(id) {
  getProductById(id);
  return remove(COLLECTION, id);
}
