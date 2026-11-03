-- ============================================================
-- Árbol de categorías, URLs amigables (slugs), complementarios,
-- banners, newsletter e imágenes optimizadas
-- ============================================================

-- Función para generar slugs a partir de texto (sin tildes, minúsculas, guiones)
CREATE OR REPLACE FUNCTION jarstore_slugify(txt TEXT) RETURNS TEXT AS $$
    SELECT COALESCE(NULLIF(trim(BOTH '-' FROM regexp_replace(
        lower(translate(txt,
            'ÁÀÄÂÃáàäâãÉÈËÊéèëêÍÌÏÎíìïîÓÒÖÔÕóòöôõÚÙÜÛúùüûÑñÇç',
            'AAAAAaaaaaEEEEeeeeIIIIiiiiOOOOOoooooUUUUuuuuNnCc')),
        '[^a-z0-9]+', '-', 'g')), ''), 'item')
$$ LANGUAGE SQL IMMUTABLE;

-- ---------- Categorías: jerarquía, slug, orden e icono ----------
ALTER TABLE categorias ADD COLUMN parent_id UUID REFERENCES categorias(id) ON DELETE SET NULL;
ALTER TABLE categorias ADD COLUMN slug VARCHAR(120);
ALTER TABLE categorias ADD COLUMN orden INT NOT NULL DEFAULT 0;
ALTER TABLE categorias ADD COLUMN icono VARCHAR(40);
ALTER TABLE categorias ADD CONSTRAINT categorias_parent_check CHECK (parent_id IS NULL OR parent_id <> id);

-- Slugs únicos para las categorías existentes
WITH s AS (
    SELECT id, jarstore_slugify(nombre) AS base,
           ROW_NUMBER() OVER (PARTITION BY jarstore_slugify(nombre) ORDER BY created_at, id) AS n
    FROM categorias
)
UPDATE categorias c SET slug = CASE WHEN s.n = 1 THEN s.base ELSE s.base || '-' || s.n END
FROM s WHERE s.id = c.id;
ALTER TABLE categorias ALTER COLUMN slug SET NOT NULL;
CREATE UNIQUE INDEX categorias_slug_unique ON categorias (slug);
CREATE INDEX categorias_parent_idx ON categorias (parent_id);

-- ---------- Productos: slug permanente ----------
ALTER TABLE productos ADD COLUMN slug VARCHAR(160);
WITH s AS (
    SELECT id, left(jarstore_slugify(nombre), 140) AS base,
           ROW_NUMBER() OVER (PARTITION BY left(jarstore_slugify(nombre), 140) ORDER BY created_at, id) AS n
    FROM productos
)
UPDATE productos p SET slug = CASE WHEN s.n = 1 THEN s.base ELSE s.base || '-' || s.n END
FROM s WHERE s.id = p.id;
ALTER TABLE productos ALTER COLUMN slug SET NOT NULL;
CREATE UNIQUE INDEX productos_slug_unique ON productos (slug);

-- Slugs antiguos (si se renombra un producto, la URL vieja redirige con 301)
CREATE TABLE producto_slugs_antiguos (
    slug VARCHAR(160) PRIMARY KEY,
    producto_id UUID NOT NULL REFERENCES productos(id) ON DELETE CASCADE,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- ---------- Productos complementarios (los elige el vendedor) ----------
CREATE TABLE producto_complementos (
    producto_id UUID NOT NULL REFERENCES productos(id) ON DELETE CASCADE,
    complemento_id UUID NOT NULL REFERENCES productos(id) ON DELETE CASCADE,
    orden INT NOT NULL DEFAULT 0,
    PRIMARY KEY (producto_id, complemento_id),
    CONSTRAINT producto_complementos_self_check CHECK (producto_id <> complemento_id)
);

-- ---------- Imágenes optimizadas: tamaños generados ----------
-- [{"w": 640, "url": "...", "type": "image/webp"}, ...]
ALTER TABLE product_images ADD COLUMN variantes JSONB NOT NULL DEFAULT '[]'::jsonb;
ALTER TABLE product_variant_images ADD COLUMN variantes JSONB NOT NULL DEFAULT '[]'::jsonb;

-- Registro de archivos subidos (para poder borrarlos del almacenamiento)
CREATE TABLE uploads (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id UUID REFERENCES users(id) ON DELETE SET NULL,
    proveedor VARCHAR(20) NOT NULL,            -- local | cloudinary
    clave VARCHAR(300) NOT NULL,               -- ruta local o public_id de Cloudinary
    url VARCHAR(500) NOT NULL UNIQUE,
    variantes JSONB NOT NULL DEFAULT '[]'::jsonb,
    ancho INT,
    alto INT,
    bytes BIGINT,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- ---------- Banners y promociones administrables ----------
CREATE TABLE banners (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    ubicacion VARCHAR(20) NOT NULL DEFAULT 'hero',   -- hero | promo
    titulo VARCHAR(160) NOT NULL,
    subtitulo VARCHAR(300),
    texto_boton VARCHAR(60),
    enlace VARCHAR(500),
    imagen_url VARCHAR(500),
    imagen_variantes JSONB NOT NULL DEFAULT '[]'::jsonb,
    imagen_movil_url VARCHAR(500),
    tema VARCHAR(10) NOT NULL DEFAULT 'claro',       -- claro | oscuro
    orden INT NOT NULL DEFAULT 0,
    activo BOOLEAN NOT NULL DEFAULT TRUE,
    desde TIMESTAMP,
    hasta TIMESTAMP,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CONSTRAINT banners_ubicacion_check CHECK (ubicacion IN ('hero', 'promo')),
    CONSTRAINT banners_tema_check CHECK (tema IN ('claro', 'oscuro')),
    CONSTRAINT banners_vigencia_check CHECK (desde IS NULL OR hasta IS NULL OR hasta > desde)
);

-- ---------- Newsletter con doble confirmación ----------
CREATE TABLE newsletter (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    email VARCHAR(255) NOT NULL UNIQUE,
    token VARCHAR(64) NOT NULL UNIQUE,
    confirmado_at TIMESTAMP,
    baja_at TIMESTAMP,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- ---------- Pagos: referencias del proveedor ----------
ALTER TABLE pagos ADD COLUMN proveedor VARCHAR(30);
CREATE UNIQUE INDEX pagos_proveedor_ref_unique ON pagos (proveedor, referencia_externa)
    WHERE proveedor IS NOT NULL AND referencia_externa IS NOT NULL;
ALTER TABLE ordenes ADD COLUMN pago_proveedor VARCHAR(30);
