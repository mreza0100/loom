import { ValidationError } from '../errors.js';
import { createLogger } from '../utils/logger.js';

const log = createLogger('discount');

const COUPONS = {
  'SAVE10':   { type: 'percentage', value: 10 },
  'SAVE20':   { type: 'percentage', value: 20 },
  'HALF':     { type: 'percentage', value: 50 },
  'FLAT5':    { type: 'fixed',      value: 5 },
  'FLAT15':   { type: 'fixed',      value: 15 },
  'WELCOME':  { type: 'percentage', value: 15 },
};

export function validateCoupon(couponCode) {
  const code = couponCode.toUpperCase().trim();
  const coupon = COUPONS[code];
  if (!coupon) throw new ValidationError('couponCode', `Invalid coupon: ${couponCode}`);
  return { code, ...coupon };
}

export function applyCoupon(cartItems, couponCode) {
  const coupon = validateCoupon(couponCode);
  const subtotal = cartItems.reduce((sum, item) => sum + item.price * item.quantity, 0);

  if (subtotal === 0) {
    throw new ValidationError('cart', 'Cannot apply coupon to empty cart');
  }

  let discount;
  if (coupon.type === 'percentage') {
    discount = subtotal * (coupon.value / 100);
  } else {
    discount = Math.min(coupon.value, subtotal);
  }

  discount = Math.round(discount * 100) / 100;
  const total = Math.round((subtotal - discount) * 100) / 100;

  log.info(`Coupon ${coupon.code} applied`, { subtotal, discount, total });
  return { subtotal, discount, total, couponCode: coupon.code, couponType: coupon.type };
}
