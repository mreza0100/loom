import { findById, findAll, insert, update } from '../database.js';
import { getUserById } from './user.js';
import { getProductById, updateStock } from './product.js';
import { NotFoundError } from '../errors.js';
import { createLogger } from '../utils/logger.js';

const log = createLogger('order-model');
const COLLECTION = 'orders';

export function createOrder(userId, items, discount = null) {
  const user = getUserById(userId);

  let subtotal = 0;
  const orderItems = items.map(item => {
    const product = getProductById(item.productId);
    const itemSubtotal = product.price * item.quantity;
    subtotal += itemSubtotal;
    return {
      productId: item.productId,
      name: product.name,
      price: product.price,
      quantity: item.quantity,
      subtotal: itemSubtotal,
    };
  });

  const discountAmount = discount ? discount.discount : 0;
  const total = Math.round((subtotal - discountAmount) * 100) / 100;

  const order = insert(COLLECTION, {
    userId,
    userName: user.name,
    items: orderItems,
    subtotal: Math.round(subtotal * 100) / 100,
    discount: discount ? { code: discount.couponCode, type: discount.couponType, amount: discountAmount } : null,
    total,
    status: 'pending',
  });

  for (const item of items) {
    updateStock(item.productId, -item.quantity);
  }

  log.info(`Order created for ${user.name}`, { total: order.total, itemCount: items.length });
  return order;
}

export function getOrderById(id) {
  const order = findById(COLLECTION, id);
  if (!order) throw new NotFoundError('Order', id);
  return order;
}

export function getOrdersByUser(userId) {
  return findAll(COLLECTION).filter(o => o.userId === userId);
}

export function updateOrderStatus(id, status) {
  const order = getOrderById(id);
  log.info(`Order ${id} status: ${order.status} -> ${status}`);
  return update(COLLECTION, id, { status });
}

export function cancelOrder(id) {
  const order = getOrderById(id);
  if (order.status === 'shipped') throw new Error('Cannot cancel shipped order');

  for (const item of order.items) {
    updateStock(item.productId, item.quantity);
  }

  return updateOrderStatus(id, 'cancelled');
}
