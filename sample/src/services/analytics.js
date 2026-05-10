import { findAll } from '../database.js';
import { createLogger } from '../utils/logger.js';

const log = createLogger('analytics');

export function getOrderTotalsByUser() {
  const orders = findAll('orders');
  const totals = {};

  for (const order of orders) {
    if (!totals[order.userId]) {
      totals[order.userId] = { userId: order.userId, userName: order.userName, totalSpent: 0, orderCount: 0 };
    }
    totals[order.userId].totalSpent = Math.round((totals[order.userId].totalSpent + order.total) * 100) / 100;
    totals[order.userId].orderCount++;
  }

  return Object.values(totals).sort((a, b) => b.totalSpent - a.totalSpent);
}

export function getTopSellingProducts(limit = 10) {
  const orders = findAll('orders');
  const products = findAll('products');
  const sales = {};

  for (const order of orders) {
    for (const item of order.items) {
      if (!sales[item.productId]) {
        sales[item.productId] = { productId: item.productId, name: item.name, unitsSold: 0, revenue: 0 };
      }
      sales[item.productId].unitsSold += item.quantity;
      sales[item.productId].revenue = Math.round((sales[item.productId].revenue + item.subtotal) * 100) / 100;
    }
  }

  for (const product of products) {
    if (!sales[product.id]) {
      sales[product.id] = { productId: product.id, name: product.name, unitsSold: 0, revenue: 0 };
    }
    sales[product.id].currentStock = product.stock;
  }

  return Object.values(sales).sort((a, b) => b.revenue - a.revenue).slice(0, limit);
}

export function generateReport() {
  const orders = findAll('orders');
  const products = findAll('products');
  const users = findAll('users');

  const totalRevenue = orders.reduce((sum, o) => sum + o.total, 0);
  const totalDiscount = orders.reduce((sum, o) => sum + (o.discount?.amount || 0), 0);

  const statusBreakdown = {};
  for (const order of orders) {
    statusBreakdown[order.status] = (statusBreakdown[order.status] || 0) + 1;
  }

  log.info('Report generated', { orders: orders.length, products: products.length, users: users.length });

  return {
    summary: {
      totalOrders: orders.length,
      totalRevenue: Math.round(totalRevenue * 100) / 100,
      totalDiscount: Math.round(totalDiscount * 100) / 100,
      averageOrderValue: orders.length > 0 ? Math.round((totalRevenue / orders.length) * 100) / 100 : 0,
      totalProducts: products.length,
      totalUsers: users.length,
    },
    ordersByStatus: statusBreakdown,
    topProducts: getTopSellingProducts(5),
    topCustomers: getOrderTotalsByUser().slice(0, 5),
  };
}
