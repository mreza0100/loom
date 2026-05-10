import { createLogger } from '../utils/logger.js';

const log = createLogger('pricing');

export class PricingStrategy {
  constructor(name) {
    this.name = name;
  }

  calculate(subtotal) {
    throw new Error(`${this.name}: calculate() must be overridden`);
  }

  describe() {
    return `Strategy: ${this.name}`;
  }
}

export class PercentageDiscount extends PricingStrategy {
  constructor(percentage) {
    super(`${percentage}% off`);
    this.percentage = percentage;
  }

  calculate(subtotal) {
    const discount = subtotal * (this.percentage / 100);
    log.info(`Applied ${this.percentage}% discount`, { subtotal, discount });
    return Math.round(discount * 100) / 100;
  }
}

export class FixedDiscount extends PricingStrategy {
  constructor(amount) {
    super(`$${amount} off`);
    this.amount = amount;
  }

  calculate(subtotal) {
    const discount = Math.min(this.amount, subtotal);
    log.info(`Applied fixed $${this.amount} discount`, { subtotal, discount });
    return Math.round(discount * 100) / 100;
  }
}

export class TieredDiscount extends PricingStrategy {
  constructor(tiers) {
    super('tiered');
    this.tiers = tiers.sort((a, b) => b.minAmount - a.minAmount);
  }

  calculate(subtotal) {
    for (const tier of this.tiers) {
      if (subtotal >= tier.minAmount) {
        const strategy = tier.type === 'percentage'
          ? new PercentageDiscount(tier.value)
          : new FixedDiscount(tier.value);
        return strategy.calculate(subtotal);
      }
    }
    return 0;
  }
}

export function createStrategy(type, config) {
  switch (type) {
    case 'percentage': return new PercentageDiscount(config.value);
    case 'fixed': return new FixedDiscount(config.value);
    case 'tiered': return new TieredDiscount(config.tiers);
    default: throw new Error(`Unknown pricing strategy: ${type}`);
  }
}
