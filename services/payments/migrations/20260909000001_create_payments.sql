CREATE TABLE payments (
    id              UUID PRIMARY KEY,
    -- Um pagamento por pedido: reentregas do evento da saga não cobram duas vezes.
    order_id        UUID NOT NULL UNIQUE,
    amount_cents    BIGINT NOT NULL CHECK (amount_cents > 0),
    status          TEXT NOT NULL,
    failure_reason  TEXT,
    created_at      TIMESTAMPTZ NOT NULL,
    updated_at      TIMESTAMPTZ NOT NULL
);

-- Pagamentos que ficaram pendentes (gateway fora) são o alvo do reprocessamento.
CREATE INDEX payments_status_idx ON payments (status);
