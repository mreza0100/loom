import { getProductById } from '../models/product.js';
import { getUserById } from '../models/user.js';
import { sendNotification } from './notification.js';
import { NotFoundError, ValidationError } from '../errors.js';
import { createLogger } from '../utils/logger.js';
import { checkAvailability } from './inventory.js';

const log = createLogger('wishlist');

const wishlists = new Map();

export function getWishlist(userId) {
  getUserById(userId);
  if (!wishlists.has(userId)) wishlists.set(userId, []);
  return wishlists.get(userId);
}

export function addToWishlist(userId, productId) {
  const product = getProductById(productId);
  const wishlist = getWishlist(userId);

  if (wishlist.find(item => item.productId === productId)) {
    throw new ValidationError('wishlist', `Product ${productId} already in wishlist`);
  }

  const item = {
    productId,
    name: product.name,
    price: product.price,
    addedAt: new Date().toISOString(),
  };

  wishlist.push(item);
  log.info(`Added to wishlist`, { userId, productId });
  return wishlist;
}

export function removeFromWishlist(userId, productId) {
  const wishlist = getWishlist(userId);
  const index = wishlist.findIndex(item => item.productId === productId);
  if (index === -1) throw new NotFoundError('Wishlist item', productId);
  wishlist.splice(index, 1);
  log.info(`Removed from wishlist`, { userId, productId });
  return wishlist;
}

export function moveToCart(userId, productId, addToCartFn) {
  removeFromWishlist(userId, productId);
  return addToCartFn(userId, productId, 1);
}

export function getWishlistAvailability(userId) {
  const wishlist = getWishlist(userId);
  const productIds = wishlist.map(item => item.productId);
  return checkAvailability(productIds);
}

export function notifyWishlistPriceDrop(userId, productId, oldPrice, newPrice) {
  if (newPrice >= oldPrice) return;
  const wishlist = getWishlist(userId);
  const item = wishlist.find(i => i.productId === productId);
  if (!item) return;

  sendNotification(userId, 'price_drop', {
    productName: item.name,
    oldPrice,
    newPrice,
    savings: Math.round((oldPrice - newPrice) * 100) / 100,
  });

  item.price = newPrice;
  log.info(`Price drop notification sent`, { userId, productId, oldPrice, newPrice });
}
