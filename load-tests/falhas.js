import exec from 'k6/execution';
import {
  addCatalogToxic,
  configureGateway,
  createOrder,
  durationMs,
  phaseAt,
  resetToxiproxy,
  seedCatalog,
  setCatalogProxyEnabled,
  sloThresholds,
  waitForSaga,
} from './lib/marketplace.js';

const FALHA = __ENV.FALHA || 'gateway-indisponivel';
const SERVICO = __ENV.SERVICO || '';
const RATE = Number(__ENV.RATE || 10);
const AQUECIMENTO = __ENV.AQUECIMENTO || '1m';
const WARMUP = __ENV.WARMUP || '1m';
const FAILURE_WINDOW = __ENV.FAILURE_WINDOW || '1m';
const RECOVERY = __ENV.RECOVERY || '3m';
const PRODUCTS = Number(__ENV.PRODUCTS || 20);
const SAGA_SAMPLE = Number(__ENV.SAGA_SAMPLE || 0);
const SAGA_TIMEOUT_MS = Number(__ENV.SAGA_TIMEOUT_MS || 180000);
const POLL_MS = Number(__ENV.POLL_MS || 500);

const HEALTHY_GATEWAY = { unavailable: false, failure_rate: 0, latency_ms: 0 };

const MODES = {
  'gateway-indisponivel': {
    inject: () => configureGateway({ unavailable: true }),
  },
  'gateway-instavel': {
    inject: () => configureGateway({ failure_rate: Number(__ENV.FAILURE_RATE || 0.5) }),
  },
  'gateway-lento': {
    inject: () => configureGateway({ latency_ms: Number(__ENV.LATENCY_MS || 2000) }),
  },
  'catalogo-lento': {
    inject: () =>
      addCatalogToxic('latencia', 'latency', {
        latency: Number(__ENV.CATALOG_LATENCY_MS || 2000),
        jitter: 0,
      }),
  },
  'catalogo-indisponivel': {
    inject: () => setCatalogProxyEnabled(false),
  },
  'catalogo-instavel': {
    inject: () =>
      addCatalogToxic('reset', 'reset_peer', { timeout: 0 }, Number(__ENV.TOXICITY || 0.3)),
  },
  'servico-parado': { external: true },
};

const MODE = MODES[FALHA];
if (!MODE) {
  throw new Error(`FALHA desconhecida: ${FALHA} (opções: ${Object.keys(MODES).join(', ')})`);
}
if (MODE.external && !SERVICO) {
  throw new Error('FALHA=servico-parado exige SERVICO; rode pelo load-tests/experimento.ps1');
}

const PLAN = [
  { duration: AQUECIMENTO, phase: 'aquecimento' },
  { duration: WARMUP, phase: 'antes' },
  { duration: FAILURE_WINDOW, phase: 'falha' },
  { duration: RECOVERY, phase: 'depois' },
];
const FAILURE_START_MS = durationMs(AQUECIMENTO) + durationMs(WARMUP);
const FAILURE_END_MS = FAILURE_START_MS + durationMs(FAILURE_WINDOW);
const TOTAL_MS = FAILURE_END_MS + durationMs(RECOVERY);

export const options = {
  scenarios: {
    compras: {
      executor: 'constant-arrival-rate',
      exec: 'comprar',
      rate: RATE,
      timeUnit: '1s',
      duration: `${TOTAL_MS}ms`,
      preAllocatedVUs: RATE * 5,
      maxVUs: Math.max(100, RATE * 40),
    },
    inicio_da_falha: {
      executor: 'shared-iterations',
      exec: 'iniciarFalha',
      vus: 1,
      iterations: 1,
      startTime: `${FAILURE_START_MS}ms`,
      maxDuration: '30s',
    },
    fim_da_falha: {
      executor: 'shared-iterations',
      exec: 'encerrarFalha',
      vus: 1,
      iterations: 1,
      startTime: `${FAILURE_END_MS}ms`,
      maxDuration: '30s',
    },
  },
  thresholds: Object.assign(
    sloThresholds(
      ['antes', 'falha', 'depois'],
      { falha: SAGA_TIMEOUT_MS, depois: SAGA_TIMEOUT_MS },
      SAGA_SAMPLE > 0,
    ),
    { 'checks{tipo:controle}': ['rate==1'] },
  ),
};

function restoreAll() {
  configureGateway(HEALTHY_GATEWAY);
  resetToxiproxy();
}

export function setup() {
  restoreAll();
  return { productIds: seedCatalog(PRODUCTS, 1000000) };
}

export function comprar(data) {
  const tags = { phase: phaseAt(PLAN, Date.now() - exec.scenario.startTime) };
  const order = createOrder(data.productIds, tags);
  if (order && Math.random() < SAGA_SAMPLE) {
    waitForSaga(order, SAGA_TIMEOUT_MS, POLL_MS, tags);
  }
}

export function iniciarFalha() {
  console.log(`>>> FALHA_INICIO falha=${FALHA} servico=${SERVICO} utc=${new Date().toISOString()}`);
  if (!MODE.external) {
    MODE.inject();
  }
}

export function encerrarFalha() {
  console.log(`>>> FALHA_FIM falha=${FALHA} servico=${SERVICO} utc=${new Date().toISOString()}`);
  if (!MODE.external) {
    restoreAll();
  }
}

export function teardown() {
  restoreAll();
}
