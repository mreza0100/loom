import { getProductById } from '../models/product.js';
import { MAX_CART_ITEMS } from '../config.js';
import { validateQuantity } from '../utils/validator.js';
import { NotFoundError, ValidationError } from '../errors.js';
import { createLogger } from '../utils/logger.js';

const log = createLogger('cart');

const carts = new Map();

export function getCart(userId) {
  if (!carts.has(userId)) carts.set(userId, []);
  return carts.get(userId);
}

export function addToCart(userId, productId, quantity = 1) {
  quantity = validateQuantity(quantity);
  const product = getProductById(productId);
  const cart = getCart(userId);

  if (cart.length >= MAX_CART_ITEMS) {
    throw new ValidationError('cart', `Maximum ${MAX_CART_ITEMS} items allowed`);
  }

  const existing = cart.find(item => item.productId === productId);
  if (existing) {
    existing.quantity += quantity;
    log.info(`Updated cart item quantity`, { userId, productId, newQty: existing.quantity });
  } else {
    cart.push({ productId, name: product.name, price: product.price, quantity });
    log.info(`Added to cart`, { userId, productId });
  }

  return cart;
}

export function removeFromCart(userId, productId) {
  const cart = getCart(userId);
  const index = cart.findIndex(item => item.productId === productId);
  if (index === -1) throw new NotFoundError('Cart item', productId);
  cart.splice(index, 1);
  log.info(`Removed from cart`, { userId, productId });
  return cart;
}

export function clearCart(userId) {
  carts.set(userId, []);
  log.info(`Cart cleared`, { userId });
  return [];
}

export function getCartTotal(userId) {
  const cart = getCart(userId);
  return cart.reduce((sum, item) => sum + item.price * item.quantity, 0);
}
