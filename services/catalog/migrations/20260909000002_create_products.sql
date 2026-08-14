CREATE TABLE products (
    id           UUID PRIMARY KEY,
    seller_id    UUID NOT NULL REFERENCES sellers (id),
    name         TEXT NOT NULL,
    description  TEXT,
    -- Centavos, espelhando domain::Money: nunca NUMERIC/FLOAT para dinheiro.
    price_cents  BIGINT NOT NULL CHECK (price_cents > 0),
    active       BOOLEAN NOT NULL DEFAULT TRUE,
    created_at   TIMESTAMPTZ NOT NULL,
    updated_at   TIMESTAMPTZ NOT NULL
);

CREATE INDEX products_seller_id_idx ON products (seller_id);
