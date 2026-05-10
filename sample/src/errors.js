export class AppError extends Error {
  constructor(message, statusCode = 500) {
    super(message);
    this.statusCode = statusCode;
    this.name = this.constructor.name;
  }
}

export class NotFoundError extends AppError {
  constructor(resource, id) {
    super(`${resource} not found: ${id}`, 404);
  }
}

export class ValidationError extends AppError {
  constructor(field, reason) {
    super(`Validation failed for ${field}: ${reason}`, 400);
    this.field = field;
  }
}

export class AuthenticationError extends AppError {
  constructor(message = 'Invalid credentials') {
    super(message, 401);
  }
}

export class PaymentError extends AppError {
  constructor(orderId, reason) {
    super(`Payment failed for order ${orderId}: ${reason}`, 402);
    this.orderId = orderId;
  }
}
