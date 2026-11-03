-- Peso y dimensiones de los productos (para cotizar envíos por courier y por tamaño/peso)
ALTER TABLE productos ADD COLUMN peso_gramos INT;
ALTER TABLE productos ADD COLUMN largo_cm INT;
ALTER TABLE productos ADD COLUMN ancho_cm INT;
ALTER TABLE productos ADD COLUMN alto_cm INT;
ALTER TABLE productos ADD CONSTRAINT productos_medidas_check CHECK (
    (peso_gramos IS NULL OR peso_gramos >= 0) AND
    (largo_cm IS NULL OR largo_cm >= 0) AND
    (ancho_cm IS NULL OR ancho_cm >= 0) AND
    (alto_cm IS NULL OR alto_cm >= 0));

-- Comuna de destino elegida (código del courier) guardada en la orden
ALTER TABLE ordenes ADD COLUMN comuna_destino VARCHAR(20);
