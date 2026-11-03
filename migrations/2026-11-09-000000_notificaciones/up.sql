-- "Avísame cuando vuelva": suscripciones a reposición de stock
CREATE TABLE avisos_stock (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    producto_id UUID NOT NULL REFERENCES productos(id) ON DELETE CASCADE,
    variante_id UUID REFERENCES product_variants(id) ON DELETE CASCADE,
    email VARCHAR(255) NOT NULL,
    user_id UUID REFERENCES users(id) ON DELETE SET NULL,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    notificado_at TIMESTAMP
);
CREATE INDEX idx_avisos_stock_pendientes ON avisos_stock (producto_id, variante_id) WHERE notificado_at IS NULL;

-- Carrito abandonado: último recordatorio enviado por usuario (para no repetir)
CREATE TABLE carrito_avisos (
    user_id UUID PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    enviado_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
);
