import { createOrder, seedCatalog, sloThresholds, waitForSaga } from './lib/marketplace.js';

const RATE = Number(__ENV.RATE || 10);
const DURATION = __ENV.DURATION || '2m';
const PRODUCTS = Number(__ENV.PRODUCTS || 20);
const SAGA_TIMEOUT_MS = Number(__ENV.SAGA_TIMEOUT_MS || 15000);
const POLL_MS = Number(__ENV.POLL_MS || 100);

export const options = {
  scenarios: {
    baseline: {
      executor: 'constant-arrival-rate',
      rate: RATE,
      timeUnit: '1s',
      duration: DURATION,
      preAllocatedVUs: Math.max(10, RATE * 2),
      maxVUs: Math.max(50, RATE * 10),
    },
  },
  thresholds: sloThresholds([]),
};

export function setup() {
  return { productIds: seedCatalog(PRODUCTS, 1000000) };
}

export default function (data) {
  const order = createOrder(data.productIds);
  if (order) {
    waitForSaga(order, SAGA_TIMEOUT_MS, POLL_MS);
  }
}
