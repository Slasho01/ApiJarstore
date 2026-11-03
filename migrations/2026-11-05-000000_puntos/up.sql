-- ============================================================
-- Programa de puntos: se ganan al pagar pedidos y se canjean como descuento
-- ============================================================

-- Libro de movimientos: el saldo de cada usuario es la suma de `puntos`.
CREATE TABLE puntos_movimientos (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    orden_id UUID REFERENCES ordenes(id) ON DELETE SET NULL,
    -- ganados | canjeados | devueltos (canje de pedido cancelado) | revertidos (ganados de pedido cancelado) | ajuste
    tipo VARCHAR(20) NOT NULL,
    puntos INT NOT NULL,
    descripcion VARCHAR(255),
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CONSTRAINT puntos_tipo_check CHECK (tipo IN ('ganados', 'canjeados', 'devueltos', 'revertidos', 'ajuste')),
    CONSTRAINT puntos_no_cero CHECK (puntos <> 0)
);
CREATE INDEX puntos_user_idx ON puntos_movimientos (user_id, created_at DESC);
-- Cada pedido gana / canjea / devuelve / revierte como máximo una vez (idempotencia)
CREATE UNIQUE INDEX puntos_orden_tipo_unique ON puntos_movimientos (orden_id, tipo)
    WHERE orden_id IS NOT NULL AND tipo <> 'ajuste';

ALTER TABLE ordenes ADD COLUMN puntos_usados INT NOT NULL DEFAULT 0;
ALTER TABLE ordenes ADD COLUMN descuento_puntos BIGINT NOT NULL DEFAULT 0;
ALTER TABLE ordenes ADD COLUMN puntos_ganados INT NOT NULL DEFAULT 0;
ALTER TABLE ordenes ADD CONSTRAINT ordenes_puntos_check CHECK (puntos_usados >= 0 AND descuento_puntos >= 0 AND puntos_ganados >= 0);

-- Reglas del programa (editables en Admin → Tienda)
INSERT INTO configuracion (clave, valor) VALUES ('puntos', '{
    "activo": true,
    "nombre": "Puntos JarStore",
    "monto_base": 1000,
    "puntos_por_monto": 1,
    "valor_punto": 10,
    "minimo_canje": 100,
    "maximo_porcentaje": 50
}'::jsonb) ON CONFLICT (clave) DO NOTHING;
