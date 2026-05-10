import { PAYMENT_RETRY_LIMIT } from '../config.js';
import { getOrderById, updateOrderStatus } from '../models/order.js';
import { PaymentError } from '../errors.js';
import { sendNotification } from './notification.js';
import { createLogger } from '../utils/logger.js';

const log = createLogger('payment');

export async function processPayment(orderId, paymentMethod) {
  const order = getOrderById(orderId);
  log.info(`Processing payment for order ${orderId}`, { total: order.total, method: paymentMethod });

  let lastError;
  for (let attempt = 1; attempt <= PAYMENT_RETRY_LIMIT; attempt++) {
    try {
      const result = await chargePayment(order.total, paymentMethod);
      updateOrderStatus(orderId, 'paid');
      sendNotification(order.userId, 'payment_success', {
        orderId,
        amount: order.total,
        transactionId: result.transactionId,
      });
      log.info(`Payment successful`, { orderId, transactionId: result.transactionId });
      return result;
    } catch (err) {
      lastError = err;
      log.warn(`Payment attempt ${attempt}/${PAYMENT_RETRY_LIMIT} failed`, { orderId, error: err.message });
    }
  }

  updateOrderStatus(orderId, 'payment_failed');
  sendNotification(order.userId, 'payment_failed', { orderId });
  throw new PaymentError(orderId, lastError.message);
}

async function chargePayment(amount, method) {
  await new Promise(resolve => setTimeout(resolve, 100));
  if (Math.random() < 0.1) throw new Error('Gateway timeout');
  return {
    transactionId: `txn_${Date.now().toString(36)}`,
    amount,
    method,
    status: 'charged',
  };
}

export function refundPayment(orderId) {
  const order = getOrderById(orderId);
  if (order.status !== 'paid') throw new PaymentError(orderId, 'Order not in paid status');
  log.info(`Refund issued for order ${orderId}`, { amount: order.total });
  updateOrderStatus(orderId, 'refunded');
  sendNotification(order.userId, 'refund_issued', { orderId, amount: order.total });
  return { orderId, refundedAmount: order.total };
}
