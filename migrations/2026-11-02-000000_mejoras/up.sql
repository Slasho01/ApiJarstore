-- ============================================================
-- Mejoras de integridad, índices y columnas nuevas
-- ============================================================

-- Usuarios: el código usa 'user', no 'buyer'
UPDATE users SET role = 'user' WHERE role NOT IN ('admin', 'seller', 'user');
ALTER TABLE users ALTER COLUMN role SET DEFAULT 'user';
ALTER TABLE users ADD CONSTRAINT users_role_check CHECK (role IN ('admin', 'seller', 'user'));

-- Categorías: nombre único (sin distinguir mayúsculas)
CREATE UNIQUE INDEX categorias_nombre_unique ON categorias (LOWER(nombre));

-- Productos
ALTER TABLE productos ADD CONSTRAINT productos_precio_check CHECK (precio >= 0);
ALTER TABLE productos ADD CONSTRAINT productos_stock_check CHECK (stock >= 0);
ALTER TABLE productos ALTER COLUMN categoria_id DROP NOT NULL;
-- Borrar una categoría no debe borrar sus productos
ALTER TABLE productos DROP CONSTRAINT productos_categoria_id_fkey;
ALTER TABLE productos ADD CONSTRAINT productos_categoria_id_fkey
    FOREIGN KEY (categoria_id) REFERENCES categorias(id) ON DELETE SET NULL;
CREATE UNIQUE INDEX productos_sku_unique ON productos (sku) WHERE sku IS NOT NULL;
CREATE INDEX productos_categoria_idx ON productos (categoria_id);
CREATE INDEX productos_user_idx ON productos (user_id);

-- Variantes
ALTER TABLE product_variants ALTER COLUMN producto_id SET NOT NULL;
ALTER TABLE product_variants ADD CONSTRAINT product_variants_stock_check CHECK (stock >= 0);
CREATE INDEX product_variants_producto_idx ON product_variants (producto_id);

-- Imágenes: id autogenerado y fechas obligatorias
ALTER TABLE product_images ALTER COLUMN id SET DEFAULT gen_random_uuid();
UPDATE product_images SET is_main = FALSE WHERE is_main IS NULL;
UPDATE product_images SET created_at = CURRENT_TIMESTAMP WHERE created_at IS NULL;
ALTER TABLE product_images ALTER COLUMN is_main SET NOT NULL;
ALTER TABLE product_images ALTER COLUMN created_at SET NOT NULL;
CREATE INDEX product_images_product_idx ON product_images (product_id);

ALTER TABLE product_variant_images ALTER COLUMN id SET DEFAULT gen_random_uuid();
UPDATE product_variant_images SET is_main = FALSE WHERE is_main IS NULL;
UPDATE product_variant_images SET created_at = CURRENT_TIMESTAMP WHERE created_at IS NULL;
ALTER TABLE product_variant_images ALTER COLUMN is_main SET NOT NULL;
ALTER TABLE product_variant_images ALTER COLUMN created_at SET NOT NULL;
CREATE INDEX product_variant_images_variant_idx ON product_variant_images (variant_id);

-- Carrito: una sola fila por (usuario, producto, variante)
DELETE FROM carrito WHERE user_id IS NULL OR producto_id IS NULL;
DELETE FROM carrito a USING carrito b
    WHERE a.ctid < b.ctid
      AND a.user_id = b.user_id
      AND a.producto_id = b.producto_id
      AND a.variante_id IS NOT DISTINCT FROM b.variante_id;
ALTER TABLE carrito ALTER COLUMN user_id SET NOT NULL;
ALTER TABLE carrito ALTER COLUMN producto_id SET NOT NULL;
ALTER TABLE carrito ADD CONSTRAINT carrito_cantidad_check CHECK (cantidad > 0);
ALTER TABLE carrito DROP CONSTRAINT carrito_variante_id_fkey;
ALTER TABLE carrito ADD CONSTRAINT carrito_variante_id_fkey
    FOREIGN KEY (variante_id) REFERENCES product_variants(id) ON DELETE CASCADE;
CREATE UNIQUE INDEX carrito_item_unique ON carrito (user_id, producto_id, variante_id) NULLS NOT DISTINCT;

-- Cupones (se crean antes porque ordenes los referencia)
UPDATE cupones SET uso_actual = 0 WHERE uso_actual IS NULL;
ALTER TABLE cupones ALTER COLUMN uso_actual SET NOT NULL;
ALTER TABLE cupones ADD CONSTRAINT cupones_tipo_check
    CHECK ((descuento_porcentaje IS NULL) <> (descuento_fijo IS NULL));
ALTER TABLE cupones ADD CONSTRAINT cupones_fijo_check CHECK (descuento_fijo IS NULL OR descuento_fijo > 0);

-- Órdenes: subtotal, descuento y cupón
ALTER TABLE ordenes ADD COLUMN subtotal BIGINT NOT NULL DEFAULT 0;
ALTER TABLE ordenes ADD COLUMN descuento BIGINT NOT NULL DEFAULT 0;
ALTER TABLE ordenes ADD COLUMN cupon_id UUID REFERENCES cupones(id) ON DELETE SET NULL;
UPDATE ordenes SET subtotal = total;
CREATE INDEX ordenes_user_idx ON ordenes (user_id);
CREATE INDEX detalle_orden_orden_idx ON detalle_orden (orden_id);
-- Conservar el historial aunque se borre un producto/variante
ALTER TABLE detalle_orden DROP CONSTRAINT detalle_orden_producto_id_fkey;
ALTER TABLE detalle_orden ADD CONSTRAINT detalle_orden_producto_id_fkey
    FOREIGN KEY (producto_id) REFERENCES productos(id) ON DELETE SET NULL;
ALTER TABLE detalle_orden DROP CONSTRAINT detalle_orden_variante_id_fkey;
ALTER TABLE detalle_orden ADD CONSTRAINT detalle_orden_variante_id_fkey
    FOREIGN KEY (variante_id) REFERENCES product_variants(id) ON DELETE SET NULL;
ALTER TABLE detalle_orden ADD COLUMN nombre_producto VARCHAR(255) NOT NULL DEFAULT '';
ALTER TABLE detalle_orden ADD COLUMN nombre_variante VARCHAR(255);

-- Pagos / envíos
CREATE INDEX pagos_orden_idx ON pagos (orden_id);
CREATE UNIQUE INDEX envios_orden_unique ON envios (orden_id);

-- Reseñas: nombre sin ñ y una reseña por usuario y producto
ALTER TABLE "reseñas" RENAME TO resenas;
CREATE UNIQUE INDEX resenas_user_producto_unique ON resenas (user_id, producto_id);
CREATE INDEX resenas_producto_idx ON resenas (producto_id);

-- Wishlist / direcciones
DELETE FROM wishlist WHERE user_id IS NULL OR producto_id IS NULL;
ALTER TABLE wishlist ALTER COLUMN user_id SET NOT NULL;
ALTER TABLE wishlist ALTER COLUMN producto_id SET NOT NULL;
DELETE FROM direcciones WHERE user_id IS NULL;
ALTER TABLE direcciones ALTER COLUMN user_id SET NOT NULL;
CREATE INDEX direcciones_user_idx ON direcciones (user_id);
