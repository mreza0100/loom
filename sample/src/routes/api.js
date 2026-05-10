import { login, logout } from '../auth/auth.js';
import { authenticate, requireRole } from '../auth/middleware.js';
import { createUser, listUsers } from '../models/user.js';
import { createProduct, listProducts, getProductById } from '../models/product.js';
import { createOrder, getOrdersByUser } from '../models/order.js';
import { addToCart, removeFromCart, getCart, clearCart } from '../services/cart.js';
import { applyCoupon } from '../services/discount.js';
import { processPayment } from '../services/payment.js';
import { getNotifications } from '../services/notification.js';
import { getWishlist, addToWishlist, removeFromWishlist, moveToCart, getWishlistAvailability } from '../services/wishlist.js';
import { generateReport, getTopSellingProducts, getOrderTotalsByUser } from '../services/analytics.js';
import { createLogger } from '../utils/logger.js';

const log = createLogger('api');

export function registerRoutes(app) {
  app.post('/auth/login', (req) => login(req.body.email, req.body.password));
  app.post('/auth/logout', (req) => {
    const authed = authenticate(req);
    return logout(authed.session.token);
  });

  app.post('/users', (req) => createUser(req.body));
  app.get('/users', (req) => {
    const authed = authenticate(req);
    requireRole('admin')(authed);
    return listUsers();
  });

  app.get('/products', () => listProducts());
  app.get('/products/:id', (req) => getProductById(req.params.id));
  app.post('/products', (req) => {
    const authed = authenticate(req);
    requireRole('admin')(authed);
    return createProduct(req.body);
  });

  app.get('/cart', (req) => {
    const authed = authenticate(req);
    return getCart(authed.user.id);
  });
  app.post('/cart', (req) => {
    const authed = authenticate(req);
    return addToCart(authed.user.id, req.body.productId, req.body.quantity);
  });
  app.delete('/cart/:productId', (req) => {
    const authed = authenticate(req);
    return removeFromCart(authed.user.id, req.params.productId);
  });

  app.post('/orders', (req) => {
    const authed = authenticate(req);
    const cart = getCart(authed.user.id);
    const discount = req.body?.couponCode ? applyCoupon(cart, req.body.couponCode) : null;
    const order = createOrder(authed.user.id, cart, discount);
    clearCart(authed.user.id);
    return order;
  });
  app.get('/orders', (req) => {
    const authed = authenticate(req);
    return getOrdersByUser(authed.user.id);
  });

  app.post('/orders/:id/pay', async (req) => {
    authenticate(req);
    return processPayment(req.params.id, req.body.paymentMethod);
  });

  app.get('/notifications', (req) => {
    const authed = authenticate(req);
    return getNotifications(authed.user.id);
  });

  app.get('/analytics/report', (req) => {
    const authed = authenticate(req);
    requireRole('admin')(authed);
    return generateReport();
  });
  app.get('/analytics/top-products', (req) => {
    const authed = authenticate(req);
    requireRole('admin')(authed);
    return getTopSellingProducts(parseInt(req.query?.limit || '10', 10));
  });
  app.get('/analytics/customers', (req) => {
    const authed = authenticate(req);
    requireRole('admin')(authed);
    return getOrderTotalsByUser();
  });

  app.get('/wishlist', (req) => {
    const authed = authenticate(req);
    return getWishlist(authed.user.id);
  });
  app.post('/wishlist', (req) => {
    const authed = authenticate(req);
    return addToWishlist(authed.user.id, req.body.productId);
  });
  app.delete('/wishlist/:productId', (req) => {
    const authed = authenticate(req);
    return removeFromWishlist(authed.user.id, req.params.productId);
  });
  app.post('/wishlist/:productId/move-to-cart', (req) => {
    const authed = authenticate(req);
    return moveToCart(authed.user.id, req.params.productId, addToCart);
  });
  app.get('/wishlist/availability', (req) => {
    const authed = authenticate(req);
    return getWishlistAvailability(authed.user.id);
  });

  log.info('Routes registered');
}
