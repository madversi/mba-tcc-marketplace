CREATE TABLE reservation_outcomes (
    order_id    UUID PRIMARY KEY,
    outcome     TEXT NOT NULL CHECK (outcome IN ('RESERVED', 'REJECTED', 'COMMITTED', 'RELEASED')),
    reason      TEXT,
    updated_at  TIMESTAMPTZ NOT NULL
);
