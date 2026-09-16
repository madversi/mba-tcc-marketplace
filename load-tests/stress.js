import exec from 'k6/execution';
import {
  createOrder,
  phaseAt,
  seedCatalog,
  sloThresholds,
  stagesOf,
  waitForSaga,
} from './lib/marketplace.js';

const PEAK_RATE = Number(__ENV.PEAK_RATE || 200);
const LEVELS = (__ENV.LEVELS || '0.25,0.5,0.75,1').split(',').map(Number);
const RAMP = __ENV.RAMP || '1m';
const HOLD = __ENV.HOLD || '2m';
const PRODUCTS = Number(__ENV.PRODUCTS || 20);
const SAGA_SAMPLE = Number(__ENV.SAGA_SAMPLE || 0.1);
const SAGA_TIMEOUT_MS = Number(__ENV.SAGA_TIMEOUT_MS || 30000);
const POLL_MS = Number(__ENV.POLL_MS || 100);

const PLAN = [];
const HOLD_PHASES = [];
LEVELS.forEach((level) => {
  const target = Math.round(PEAK_RATE * level);
  PLAN.push({ target: target, duration: RAMP, phase: `rampa-${target}rps` });
  PLAN.push({ target: target, duration: HOLD, phase: `${target}rps` });
  HOLD_PHASES.push(`${target}rps`);
});
PLAN.push({ target: 0, duration: RAMP, phase: 'desaceleracao' });

export const options = {
  scenarios: {
    stress: {
      executor: 'ramping-arrival-rate',
      startRate: 0,
      timeUnit: '1s',
      stages: stagesOf(PLAN),
      preAllocatedVUs: Math.round(PEAK_RATE / 2),
      maxVUs: PEAK_RATE * 2,
    },
  },
  thresholds: sloThresholds(HOLD_PHASES),
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
