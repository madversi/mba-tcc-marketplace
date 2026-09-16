import http from 'k6/http';
import { check, fail, sleep } from 'k6';
import { Rate, Trend } from 'k6/metrics';

export const urls = {
  catalog: __ENV.CATALOG_URL || 'http://localhost:8081',
  orders: __ENV.ORDERS_URL || 'http://localhost:8082',
  inventory: __ENV.INVENTORY_URL || 'http://localhost:8083',
  payments: __ENV.PAYMENTS_URL || 'http://localhost:8084',
};

export const sagaDuration = new Trend('saga_duration', true);
export const sagaConfirmed = new Rate('saga_confirmed');

const TERMINAL = ['CONFIRMED', 'CANCELLED'];

const SLO = {
  orderFailedRate: 'rate<0.01',
  orderP95: 'p(95)<500',
  sagaConfirmedRate: 'rate>0.99',
  sagaP95: 'p(95)<3000',
};

function params(name, extraTags) {
  return {
    headers: { 'Content-Type': 'application/json' },
    tags: Object.assign({ name: name }, extraTags || {}),
  };
}

function mustSucceed(response, what) {
  if (response.status < 200 || response.status >= 300) {
    fail(`${what} falhou: HTTP ${response.status} ${response.body}`);
  }
}

function uuid() {
  return 'xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx'.replace(/[xy]/g, (c) => {
    const r = (Math.random() * 16) | 0;
    return (c === 'x' ? r : (r & 0x3) | 0x8).toString(16);
  });
}

export function durationMs(value) {
  const units = { ms: 1, s: 1000, m: 60000, h: 3600000 };
  const pattern = /(\d+)(ms|s|m|h)/g;
  let total = 0;
  let match;
  while ((match = pattern.exec(value)) !== null) {
    total += Number(match[1]) * units[match[2]];
  }
  return total;
}

export function stagesOf(plan) {
  return plan.map((step) => ({ target: step.target, duration: step.duration }));
}

export function phaseAt(plan, elapsedMs) {
  let end = 0;
  for (let i = 0; i < plan.length; i++) {
    end += durationMs(plan[i].duration);
    if (elapsedMs < end) {
      return plan[i].phase;
    }
  }
  return plan[plan.length - 1].phase;
}

export function sloThresholds(phases, sagaDurationLimitsMs) {
  const limits = sagaDurationLimitsMs || {};
  const scopes = phases.length ? phases : [null];
  const thresholds = {};
  scopes.forEach((phase) => {
    const order = phase ? `{name:POST /orders,phase:${phase}}` : '{name:POST /orders}';
    const saga = phase ? `{phase:${phase}}` : '';
    thresholds[`http_req_failed${order}`] = [SLO.orderFailedRate];
    thresholds[`http_req_duration${order}`] = [SLO.orderP95];
    thresholds[`saga_confirmed${saga}`] = [SLO.sagaConfirmedRate];
    thresholds[`saga_duration${saga}`] = [
      phase && limits[phase] ? `p(95)<${limits[phase]}` : SLO.sagaP95,
    ];
  });
  return thresholds;
}

export function configureGateway(settings) {
  const response = http.patch(
    `${urls.payments}/admin/gateway`,
    JSON.stringify(settings),
    params('PATCH /admin/gateway', { tipo: 'controle' }),
  );
  return check(
    response,
    { 'gateway reconfigurado (200)': (r) => r.status === 200 },
    { tipo: 'controle' },
  );
}

export function seedCatalog(productCount, stockPerProduct) {
  const seller = http.post(
    `${urls.catalog}/sellers`,
    JSON.stringify({ name: 'Loja k6', email: `k6-${uuid()}@carga.test` }),
    params('setup'),
  );
  mustSucceed(seller, 'criar vendedor');

  const productIds = [];
  for (let i = 0; i < productCount; i++) {
    const product = http.post(
      `${urls.catalog}/products`,
      JSON.stringify({
        seller_id: seller.json('id'),
        name: `Produto k6 ${i + 1}`,
        price_cents: 1000 + i * 100,
      }),
      params('setup'),
    );
    mustSucceed(product, 'criar produto');

    const stock = http.put(
      `${urls.inventory}/stock/${product.json('id')}`,
      JSON.stringify({ quantity: stockPerProduct }),
      params('setup'),
    );
    mustSucceed(stock, 'carregar estoque');

    productIds.push(product.json('id'));
  }
  return productIds;
}

export function createOrder(productIds, tags) {
  const productId = productIds[Math.floor(Math.random() * productIds.length)];
  const startedAt = Date.now();
  const response = http.post(
    `${urls.orders}/orders`,
    JSON.stringify({
      buyer_id: uuid(),
      items: [{ product_id: productId, quantity: 1 }],
    }),
    params('POST /orders', tags),
  );

  const created = check(response, { 'pedido criado (201)': (r) => r.status === 201 }, tags);
  return created ? { id: response.json('id'), startedAt: startedAt } : null;
}

export function waitForSaga(order, timeoutMs, pollMs, tags) {
  const deadline = order.startedAt + timeoutMs;
  while (Date.now() < deadline) {
    const response = http.get(
      `${urls.orders}/orders/${order.id}`,
      params('GET /orders/{id}', tags),
    );
    const status = response.status === 200 ? response.json('status') : null;
    if (TERMINAL.indexOf(status) !== -1) {
      sagaDuration.add(Date.now() - order.startedAt, tags);
      sagaConfirmed.add(status === 'CONFIRMED', tags);
      return status;
    }
    sleep(pollMs / 1000);
  }
  sagaConfirmed.add(false, tags);
  return 'TIMEOUT';
}
