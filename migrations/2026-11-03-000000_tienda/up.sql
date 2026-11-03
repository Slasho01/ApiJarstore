-- ============================================================
-- Catálogo enriquecido, envíos, monedas, políticas y recuperación de contraseña
-- ============================================================

-- Productos: marca, oferta, destacado, especificaciones, compatibilidad, garantía
ALTER TABLE productos ADD COLUMN marca VARCHAR(100);
ALTER TABLE productos ADD COLUMN precio_anterior BIGINT;
ALTER TABLE productos ADD COLUMN destacado BOOLEAN NOT NULL DEFAULT FALSE;
-- Lista de pares: [{"nombre": "RAM", "valor": "16 GB"}, ...]
ALTER TABLE productos ADD COLUMN especificaciones JSONB NOT NULL DEFAULT '[]'::jsonb;
ALTER TABLE productos ADD COLUMN compatibilidad TEXT;
ALTER TABLE productos ADD COLUMN garantia_meses INT;
ALTER TABLE productos ADD CONSTRAINT productos_precio_anterior_check
    CHECK (precio_anterior IS NULL OR precio_anterior > precio);
ALTER TABLE productos ADD CONSTRAINT productos_garantia_check
    CHECK (garantia_meses IS NULL OR garantia_meses >= 0);
ALTER TABLE productos ADD CONSTRAINT productos_especificaciones_check
    CHECK (jsonb_typeof(especificaciones) = 'array');
CREATE INDEX productos_marca_idx ON productos (LOWER(marca));
CREATE INDEX productos_destacado_idx ON productos (destacado) WHERE destacado;
CREATE INDEX productos_especificaciones_idx ON productos USING GIN (especificaciones jsonb_path_ops);

-- Órdenes: comprador, envío y moneda en que se mostró la compra
ALTER TABLE ordenes ADD COLUMN costo_envio BIGINT NOT NULL DEFAULT 0;
ALTER TABLE ordenes ADD COLUMN metodo_envio VARCHAR(100);
ALTER TABLE ordenes ADD COLUMN pais VARCHAR(2);
ALTER TABLE ordenes ADD COLUMN moneda VARCHAR(3) NOT NULL DEFAULT 'CLP';
ALTER TABLE ordenes ADD COLUMN tasa_cambio DOUBLE PRECISION NOT NULL DEFAULT 1;
ALTER TABLE ordenes ADD COLUMN nombre_comprador VARCHAR(255);
ALTER TABLE ordenes ADD COLUMN email_comprador VARCHAR(255);
ALTER TABLE ordenes ADD COLUMN telefono_comprador VARCHAR(50);

-- Configuración editable desde el panel (tienda, monedas, envíos, pagos)
CREATE TABLE configuracion (
    clave VARCHAR(50) PRIMARY KEY,
    valor JSONB NOT NULL,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
);

INSERT INTO configuracion (clave, valor) VALUES
('tienda', '{
    "nombre": "JarStore",
    "descripcion": "",
    "email": "",
    "telefono": "",
    "whatsapp": "",
    "direccion": "",
    "redes": {"instagram": "", "facebook": "", "tiktok": "", "x": ""},
    "beneficios": []
}'::jsonb),
('monedas', '{
    "base": "CLP",
    "lista": [
        {"codigo": "CLP", "nombre": "Peso chileno", "simbolo": "$", "decimales": 0, "tasa": 1, "locale": "es-CL"},
        {"codigo": "VES", "nombre": "Bolívar venezolano", "simbolo": "Bs.", "decimales": 2, "tasa": null, "locale": "es-VE"}
    ]
}'::jsonb),
('envio', '{
    "paises": [
        {"codigo": "CL", "nombre": "Chile", "metodos": []},
        {"codigo": "VE", "nombre": "Venezuela", "metodos": []}
    ]
}'::jsonb),
('pagos', '{"metodos": []}'::jsonb);

-- Páginas de contenido: políticas de devolución, garantía, despacho, términos...
CREATE TABLE paginas (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    slug VARCHAR(100) NOT NULL UNIQUE,
    titulo VARCHAR(255) NOT NULL,
    contenido TEXT NOT NULL DEFAULT '',
    visible BOOLEAN NOT NULL DEFAULT FALSE,
    en_footer BOOLEAN NOT NULL DEFAULT TRUE,
    orden INT NOT NULL DEFAULT 0,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CONSTRAINT paginas_slug_check CHECK (slug ~ '^[a-z0-9]+(-[a-z0-9]+)*$')
);

-- Se crean ocultas: el admin las completa y las publica desde el panel
INSERT INTO paginas (slug, titulo, orden) VALUES
('devoluciones', 'Cambios y devoluciones', 1),
('garantia', 'Garantía', 2),
('despacho', 'Despacho y entregas', 3),
('terminos', 'Términos y condiciones', 4),
('privacidad', 'Política de privacidad', 5);

-- Recuperación de contraseña (se guarda sólo el hash del token)
CREATE TABLE password_resets (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    token_hash VARCHAR(64) NOT NULL UNIQUE,
    expires_at TIMESTAMP NOT NULL,
    used_at TIMESTAMP,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX password_resets_user_idx ON password_resets (user_id);
