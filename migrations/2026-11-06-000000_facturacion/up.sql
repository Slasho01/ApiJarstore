-- ============================================================
-- Documento tributario (boleta/factura) y datos de facturación
-- Chile pide tipo de documento; factura requiere datos. Venezuela queda sin documento.
-- ============================================================
ALTER TABLE ordenes ADD COLUMN tipo_documento VARCHAR(10);
ALTER TABLE ordenes ADD COLUMN factura_rut VARCHAR(20);
ALTER TABLE ordenes ADD COLUMN factura_razon_social VARCHAR(255);
ALTER TABLE ordenes ADD COLUMN factura_giro VARCHAR(255);
ALTER TABLE ordenes ADD COLUMN factura_direccion TEXT;
ALTER TABLE ordenes ADD COLUMN documento_url VARCHAR(500);
ALTER TABLE ordenes ADD COLUMN documento_subido_at TIMESTAMP;
ALTER TABLE ordenes ADD CONSTRAINT ordenes_tipo_documento_check
    CHECK (tipo_documento IS NULL OR tipo_documento IN ('boleta', 'factura'));
