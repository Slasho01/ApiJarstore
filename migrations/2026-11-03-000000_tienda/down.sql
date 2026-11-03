DROP TABLE IF EXISTS password_resets;
DROP TABLE IF EXISTS paginas;
DROP TABLE IF EXISTS configuracion;

ALTER TABLE ordenes DROP COLUMN telefono_comprador;
ALTER TABLE ordenes DROP COLUMN email_comprador;
ALTER TABLE ordenes DROP COLUMN nombre_comprador;
ALTER TABLE ordenes DROP COLUMN tasa_cambio;
ALTER TABLE ordenes DROP COLUMN moneda;
ALTER TABLE ordenes DROP COLUMN pais;
ALTER TABLE ordenes DROP COLUMN metodo_envio;
ALTER TABLE ordenes DROP COLUMN costo_envio;

DROP INDEX IF EXISTS productos_especificaciones_idx;
DROP INDEX IF EXISTS productos_destacado_idx;
DROP INDEX IF EXISTS productos_marca_idx;
ALTER TABLE productos DROP CONSTRAINT productos_especificaciones_check;
ALTER TABLE productos DROP CONSTRAINT productos_garantia_check;
ALTER TABLE productos DROP CONSTRAINT productos_precio_anterior_check;
ALTER TABLE productos DROP COLUMN garantia_meses;
ALTER TABLE productos DROP COLUMN compatibilidad;
ALTER TABLE productos DROP COLUMN especificaciones;
ALTER TABLE productos DROP COLUMN destacado;
ALTER TABLE productos DROP COLUMN precio_anterior;
ALTER TABLE productos DROP COLUMN marca;
