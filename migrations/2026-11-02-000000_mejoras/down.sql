DROP INDEX IF EXISTS direcciones_user_idx;
ALTER TABLE direcciones ALTER COLUMN user_id DROP NOT NULL;
ALTER TABLE wishlist ALTER COLUMN producto_id DROP NOT NULL;
ALTER TABLE wishlist ALTER COLUMN user_id DROP NOT NULL;

DROP INDEX IF EXISTS resenas_producto_idx;
DROP INDEX IF EXISTS resenas_user_producto_unique;
ALTER TABLE resenas RENAME TO "reseñas";

DROP INDEX IF EXISTS envios_orden_unique;
DROP INDEX IF EXISTS pagos_orden_idx;

ALTER TABLE detalle_orden DROP COLUMN nombre_variante;
ALTER TABLE detalle_orden DROP COLUMN nombre_producto;
ALTER TABLE detalle_orden DROP CONSTRAINT detalle_orden_variante_id_fkey;
ALTER TABLE detalle_orden ADD CONSTRAINT detalle_orden_variante_id_fkey
    FOREIGN KEY (variante_id) REFERENCES product_variants(id);
ALTER TABLE detalle_orden DROP CONSTRAINT detalle_orden_producto_id_fkey;
ALTER TABLE detalle_orden ADD CONSTRAINT detalle_orden_producto_id_fkey
    FOREIGN KEY (producto_id) REFERENCES productos(id);
DROP INDEX IF EXISTS detalle_orden_orden_idx;
DROP INDEX IF EXISTS ordenes_user_idx;
ALTER TABLE ordenes DROP COLUMN cupon_id;
ALTER TABLE ordenes DROP COLUMN descuento;
ALTER TABLE ordenes DROP COLUMN subtotal;

ALTER TABLE cupones DROP CONSTRAINT cupones_fijo_check;
ALTER TABLE cupones DROP CONSTRAINT cupones_tipo_check;
ALTER TABLE cupones ALTER COLUMN uso_actual DROP NOT NULL;

DROP INDEX IF EXISTS carrito_item_unique;
ALTER TABLE carrito DROP CONSTRAINT carrito_variante_id_fkey;
ALTER TABLE carrito ADD CONSTRAINT carrito_variante_id_fkey
    FOREIGN KEY (variante_id) REFERENCES product_variants(id);
ALTER TABLE carrito DROP CONSTRAINT carrito_cantidad_check;
ALTER TABLE carrito ALTER COLUMN producto_id DROP NOT NULL;
ALTER TABLE carrito ALTER COLUMN user_id DROP NOT NULL;

DROP INDEX IF EXISTS product_variant_images_variant_idx;
ALTER TABLE product_variant_images ALTER COLUMN created_at DROP NOT NULL;
ALTER TABLE product_variant_images ALTER COLUMN is_main DROP NOT NULL;
ALTER TABLE product_variant_images ALTER COLUMN id DROP DEFAULT;
DROP INDEX IF EXISTS product_images_product_idx;
ALTER TABLE product_images ALTER COLUMN created_at DROP NOT NULL;
ALTER TABLE product_images ALTER COLUMN is_main DROP NOT NULL;
ALTER TABLE product_images ALTER COLUMN id DROP DEFAULT;

DROP INDEX IF EXISTS product_variants_producto_idx;
ALTER TABLE product_variants DROP CONSTRAINT product_variants_stock_check;
ALTER TABLE product_variants ALTER COLUMN producto_id DROP NOT NULL;

DROP INDEX IF EXISTS productos_user_idx;
DROP INDEX IF EXISTS productos_categoria_idx;
DROP INDEX IF EXISTS productos_sku_unique;
ALTER TABLE productos DROP CONSTRAINT productos_categoria_id_fkey;
ALTER TABLE productos ADD CONSTRAINT productos_categoria_id_fkey
    FOREIGN KEY (categoria_id) REFERENCES categorias(id) ON DELETE CASCADE;
ALTER TABLE productos DROP CONSTRAINT productos_stock_check;
ALTER TABLE productos DROP CONSTRAINT productos_precio_check;

DROP INDEX IF EXISTS categorias_nombre_unique;

ALTER TABLE users DROP CONSTRAINT users_role_check;
ALTER TABLE users ALTER COLUMN role SET DEFAULT 'buyer';
