import { createLogger } from '../utils/logger.js';
import { getUserById } from '../models/user.js';

const log = createLogger('notification');

const notificationQueue = [];

const TEMPLATES = {
  payment_success: (data) => `Payment of $${data.amount} received for order ${data.orderId}`,
  payment_failed: (data) => `Payment failed for order ${data.orderId}. Please try again.`,
  refund_issued: (data) => `Refund of $${data.amount} issued for order ${data.orderId}`,
  order_shipped: (data) => `Your order ${data.orderId} has been shipped! Tracking: ${data.trackingId}`,
  welcome: (data) => `Welcome to the store, ${data.name}!`,
};

export function sendNotification(userId, type, data = {}) {
  const user = getUserById(userId);
  const template = TEMPLATES[type];
  if (!template) {
    log.warn(`Unknown notification type: ${type}`);
    return null;
  }

  const notification = {
    id: `notif_${Date.now().toString(36)}`,
    userId,
    email: user.email,
    type,
    message: template(data),
    sentAt: new Date().toISOString(),
    read: false,
  };

  notificationQueue.push(notification);
  log.info(`Notification sent to ${user.name}`, { type });
  return notification;
}

export function getNotifications(userId) {
  return notificationQueue.filter(n => n.userId === userId);
}

export function getUnreadCount(userId) {
  return notificationQueue.filter(n => n.userId === userId && !n.read).length;
}

export function markAsRead(notificationId) {
  const notif = notificationQueue.find(n => n.id === notificationId);
  if (notif) notif.read = true;
  return notif;
}
