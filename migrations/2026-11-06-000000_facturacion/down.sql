ALTER TABLE ordenes DROP CONSTRAINT ordenes_tipo_documento_check;
ALTER TABLE ordenes DROP COLUMN documento_subido_at;
ALTER TABLE ordenes DROP COLUMN documento_url;
ALTER TABLE ordenes DROP COLUMN factura_direccion;
ALTER TABLE ordenes DROP COLUMN factura_giro;
ALTER TABLE ordenes DROP COLUMN factura_razon_social;
ALTER TABLE ordenes DROP COLUMN factura_rut;
ALTER TABLE ordenes DROP COLUMN tipo_documento;
