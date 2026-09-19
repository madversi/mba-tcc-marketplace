import exec from 'k6/execution';
import {
  createOrder,
  phaseAt,
  seedCatalog,
  sloThresholds,
  stagesOf,
  waitForSaga,
} from './lib/marketplace.js';

const BASE_RATE = Number(__ENV.BASE_RATE || 10);
const SPIKE_RATE = Number(__ENV.SPIKE_RATE || 150);
const WARMUP = __ENV.WARMUP || '1m';
const SPIKE_RAMP = __ENV.SPIKE_RAMP || '5s';
const SPIKE_HOLD = __ENV.SPIKE_HOLD || '1m';
const RECOVERY = __ENV.RECOVERY || '2m';
const PRODUCTS = Number(__ENV.PRODUCTS || 20);
const SAGA_SAMPLE = Number(__ENV.SAGA_SAMPLE || 0);
const SAGA_TIMEOUT_MS = Number(__ENV.SAGA_TIMEOUT_MS || 30000);
const POLL_MS = Number(__ENV.POLL_MS || 100);

const PLAN = [
  { target: BASE_RATE, duration: WARMUP, phase: 'antes' },
  { target: SPIKE_RATE, duration: SPIKE_RAMP, phase: 'subida' },
  { target: SPIKE_RATE, duration: SPIKE_HOLD, phase: 'pico' },
  { target: BASE_RATE, duration: SPIKE_RAMP, phase: 'descida' },
  { target: BASE_RATE, duration: RECOVERY, phase: 'depois' },
];

export const options = {
  scenarios: {
    spike: {
      executor: 'ramping-arrival-rate',
      startRate: BASE_RATE,
      timeUnit: '1s',
      stages: stagesOf(PLAN),
      preAllocatedVUs: SPIKE_RATE,
      maxVUs: SPIKE_RATE * 2,
    },
  },
  thresholds: sloThresholds(['antes', 'pico', 'depois'], {}, SAGA_SAMPLE > 0),
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
