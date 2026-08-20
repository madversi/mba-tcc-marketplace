-- Sem FK para products: o catálogo vive em outro serviço/database. A
-- consistência entre eles é responsabilidade da saga, não do banco.
CREATE TABLE stock (
    product_id  UUID PRIMARY KEY,
    available   BIGINT NOT NULL CHECK (available >= 0),
    reserved    BIGINT NOT NULL CHECK (reserved >= 0),
    updated_at  TIMESTAMPTZ NOT NULL
);
