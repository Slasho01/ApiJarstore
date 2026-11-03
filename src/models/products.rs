use crate::schema::{product_images, product_variant_images, product_variants, productos};
use chrono::NaiveDateTime;
use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Queryable, Selectable, Identifiable, Serialize, Debug, Clone)]
#[diesel(table_name = productos, check_for_backend(diesel::pg::Pg))]
pub struct Producto {
    pub id: Uuid,
    pub nombre: String,
    pub descripcion: Option<String>,
    /// Precio en la unidad mínima (centavos)
    pub precio: i64,
    pub stock: i32,
    pub sku: Option<String>,
    pub estatus: bool,
    pub categoria_id: Option<Uuid>,
    pub user_id: Option<Uuid>,
    pub marca: Option<String>,
    /// Precio "antes" (tachado). Sólo existe si es mayor que `precio`.
    pub precio_anterior: Option<i64>,
    pub destacado: bool,
    pub especificaciones: serde_json::Value,
    pub compatibilidad: Option<String>,
    pub garantia_meses: Option<i32>,
    /// URL amigable permanente: /producto/{slug}
    pub slug: String,
    /// Peso y dimensiones para cotizar envíos; None si no se definieron
    pub peso_gramos: Option<i32>,
    pub largo_cm: Option<i32>,
    pub ancho_cm: Option<i32>,
    pub alto_cm: Option<i32>,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}

/// Tamaño generado de una imagen subida (para `srcset`).
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ImagenVariante {
    pub w: u32,
    pub url: String,
    #[serde(rename = "type", default)]
    pub mime: Option<String>,
}

/// Par nombre/valor de una especificación técnica.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Especificacion {
    pub nombre: String,
    pub valor: String,
}

#[derive(Insertable, Debug)]
#[diesel(table_name = productos)]
pub struct NewProducto {
    pub nombre: String,
    pub descripcion: Option<String>,
    pub precio: i64,
    pub stock: i32,
    pub sku: Option<String>,
    pub estatus: bool,
    pub categoria_id: Option<Uuid>,
    pub user_id: Option<Uuid>,
    pub marca: Option<String>,
    pub precio_anterior: Option<i64>,
    pub destacado: bool,
    pub especificaciones: serde_json::Value,
    pub compatibilidad: Option<String>,
    pub garantia_meses: Option<i32>,
    pub slug: String,
    pub peso_gramos: Option<i32>,
    pub largo_cm: Option<i32>,
    pub ancho_cm: Option<i32>,
    pub alto_cm: Option<i32>,
}

#[derive(AsChangeset, Deserialize, Debug, Default)]
#[diesel(table_name = productos)]
pub struct ProductoUpdate {
    pub nombre: Option<String>,
    pub descripcion: Option<String>,
    pub precio: Option<i64>,
    pub stock: Option<i32>,
    pub sku: Option<String>,
    pub estatus: Option<bool>,
    pub categoria_id: Option<Uuid>,
    pub marca: Option<String>,
    pub destacado: Option<bool>,
    pub compatibilidad: Option<String>,
    pub peso_gramos: Option<i32>,
    pub largo_cm: Option<i32>,
    pub ancho_cm: Option<i32>,
    pub alto_cm: Option<i32>,
    /// Cambiar el slug guarda el anterior para redirigir (301)
    pub slug: Option<String>,
    /// Se validan aparte y se asignan en el servicio
    #[serde(skip)]
    pub especificaciones: Option<serde_json::Value>,
    #[serde(skip)]
    pub updated_at: Option<NaiveDateTime>,
}

#[derive(Queryable, Selectable, Identifiable, Associations, Serialize, Debug, Clone)]
#[diesel(belongs_to(Producto))]
#[diesel(table_name = product_variants, check_for_backend(diesel::pg::Pg))]
pub struct Variante {
    pub id: Uuid,
    pub producto_id: Uuid,
    pub nombre: String,
    pub valor: String,
    pub stock: i32,
    pub precio_extra: i64,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}

#[derive(Insertable, Debug)]
#[diesel(table_name = product_variants)]
pub struct NewVariante {
    pub producto_id: Uuid,
    pub nombre: String,
    pub valor: String,
    pub stock: i32,
    pub precio_extra: i64,
}

#[derive(AsChangeset, Deserialize, Debug)]
#[diesel(table_name = product_variants)]
pub struct VarianteUpdate {
    pub nombre: Option<String>,
    pub valor: Option<String>,
    pub stock: Option<i32>,
    pub precio_extra: Option<i64>,
    #[serde(skip)]
    pub updated_at: Option<NaiveDateTime>,
}

#[derive(Queryable, Selectable, Identifiable, Associations, Serialize, Debug)]
#[diesel(belongs_to(Producto, foreign_key = product_id))]
#[diesel(table_name = product_images, check_for_backend(diesel::pg::Pg))]
pub struct ProductImage {
    pub id: Uuid,
    pub product_id: Uuid,
    pub image_url: String,
    pub alt_text: Option<String>,
    pub is_main: bool,
    pub variantes: serde_json::Value,
    pub created_at: NaiveDateTime,
}

#[derive(Insertable, Debug)]
#[diesel(table_name = product_images)]
pub struct NewProductImage {
    pub product_id: Uuid,
    pub image_url: String,
    pub alt_text: Option<String>,
    pub is_main: bool,
    pub variantes: serde_json::Value,
}

#[derive(Queryable, Selectable, Identifiable, Associations, Serialize, Debug)]
#[diesel(belongs_to(Variante, foreign_key = variant_id))]
#[diesel(table_name = product_variant_images, check_for_backend(diesel::pg::Pg))]
pub struct VariantImage {
    pub id: Uuid,
    pub variant_id: Uuid,
    pub image_url: String,
    pub alt_text: Option<String>,
    pub is_main: bool,
    pub variantes: serde_json::Value,
    pub created_at: NaiveDateTime,
}

#[derive(Insertable, Debug)]
#[diesel(table_name = product_variant_images)]
pub struct NewVariantImage {
    pub variant_id: Uuid,
    pub image_url: String,
    pub alt_text: Option<String>,
    pub is_main: bool,
    pub variantes: serde_json::Value,
}
