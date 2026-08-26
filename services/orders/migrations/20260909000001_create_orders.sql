CREATE TABLE orders (
    id           UUID PRIMARY KEY,
    buyer_id     UUID NOT NULL,
    status       TEXT NOT NULL,
    total_cents  BIGINT NOT NULL CHECK (total_cents >= 0),
    created_at   TIMESTAMPTZ NOT NULL,
    updated_at   TIMESTAMPTZ NOT NULL
);

CREATE INDEX orders_status_idx ON orders (status);

-- product_id sem FK: o catálogo é outro serviço/database. O preço é
-- congelado aqui no momento da compra; o catálogo pode mudar depois.
CREATE TABLE order_items (
    order_id          UUID NOT NULL REFERENCES orders (id) ON DELETE CASCADE,
    product_id        UUID NOT NULL,
    quantity          BIGINT NOT NULL CHECK (quantity > 0),
    unit_price_cents  BIGINT NOT NULL CHECK (unit_price_cents > 0),
    PRIMARY KEY (order_id, product_id)
);
