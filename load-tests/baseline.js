import exec from 'k6/execution';
import {
  createOrder,
  durationMs,
  phaseAt,
  seedCatalog,
  sloThresholds,
  waitForSaga,
} from './lib/marketplace.js';

const RATE = Number(__ENV.RATE || 10);
const AQUECIMENTO = __ENV.AQUECIMENTO || '1m';
const DURATION = __ENV.DURATION || '2m';
const PRODUCTS = Number(__ENV.PRODUCTS || 20);
const SAGA_SAMPLE = Number(__ENV.SAGA_SAMPLE || 0);
const SAGA_TIMEOUT_MS = Number(__ENV.SAGA_TIMEOUT_MS || 15000);
const POLL_MS = Number(__ENV.POLL_MS || 100);

const PLAN = [
  { duration: AQUECIMENTO, phase: 'aquecimento' },
  { duration: DURATION, phase: 'medicao' },
];

export const options = {
  scenarios: {
    baseline: {
      executor: 'constant-arrival-rate',
      rate: RATE,
      timeUnit: '1s',
      duration: `${durationMs(AQUECIMENTO) + durationMs(DURATION)}ms`,
      preAllocatedVUs: Math.max(10, RATE * 2),
      maxVUs: Math.max(50, RATE * 10),
    },
  },
  thresholds: sloThresholds(['medicao'], {}, SAGA_SAMPLE > 0),
};

export function setup() {
  return { productIds: seedCatalog(PRODUCTS, 1000000) };
}

export default function (data) {
  const tags = { phase: phaseAt(PLAN, Date.now() - exec.scenario.startTime) };
  const order = createOrder(data.productIds, tags);
  if (order && Math.random() < SAGA_SAMPLE) {
    waitForSaga(order, SAGA_TIMEOUT_MS, POLL_MS, tags);
  }
}
