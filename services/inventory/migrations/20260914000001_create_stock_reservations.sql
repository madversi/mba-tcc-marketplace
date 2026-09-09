CREATE TABLE stock_reservations (
    order_id    UUID NOT NULL,
    product_id  UUID NOT NULL,
    quantity    BIGINT NOT NULL CHECK (quantity > 0),
    PRIMARY KEY (order_id, product_id)
);
