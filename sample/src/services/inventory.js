import { getProductById as getProduct } from '../models/product.js';
import { createLogger } from '../utils/logger.js';

const log = createLogger('inventory');

export function getProductById(id) {
  const product = getProduct(id);
  if (product.stock <= 0) {
    log.warn(`Product ${id} is out of stock`);
  }
  return { ...product, inStock: product.stock > 0, lowStock: product.stock < 5 };
}

export function checkAvailability(productIds) {
  return productIds.map(id => {
    const product = getProductById(id);
    return { id, name: product.name, available: product.inStock, stock: product.stock };
  });
}

export function getLowStockProducts(threshold = 5) {
  const { findAll } = require('../database.js');
  return findAll('products').filter(p => p.stock < threshold);
}
