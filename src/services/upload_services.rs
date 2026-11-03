use crate::errors::ApiResult;
use crate::models::products::ImagenVariante;
use crate::models::tienda::{NewUpload, Upload};
use crate::schema::{banners, product_images, product_variant_images, uploads};
use crate::storage::Stored;
use diesel::prelude::*;
use uuid::Uuid;

pub fn record(conn: &mut PgConnection, user_id: Uuid, s: &Stored) -> ApiResult<Upload> {
    Ok(diesel::insert_into(uploads::table)
        .values(NewUpload {
            user_id: Some(user_id),
            proveedor: s.proveedor.to_string(),
            clave: s.clave.clone(),
            url: s.url.clone(),
            variantes: serde_json::to_value(&s.variantes).unwrap_or_default(),
            ancho: Some(s.ancho as i32),
            alto: Some(s.alto as i32),
            bytes: Some(s.bytes),
        })
        .returning(Upload::as_returning())
        .get_result(conn)?)
}

/// Si la URL era una subida propia y ya nada la usa, elimina el registro y devuelve
/// los datos para borrar los archivos del almacenamiento.
pub fn release(conn: &mut PgConnection, url: &str) -> ApiResult<Option<(String, String, Vec<ImagenVariante>)>> {
    let in_use: bool = diesel::select(diesel::dsl::exists(product_images::table.filter(product_images::image_url.eq(url)))).get_result(conn)?
        || diesel::select(diesel::dsl::exists(product_variant_images::table.filter(product_variant_images::image_url.eq(url)))).get_result(conn)?
        || diesel::select(diesel::dsl::exists(banners::table.filter(banners::imagen_url.eq(url).or(banners::imagen_movil_url.eq(url))))).get_result(conn)?;
    if in_use {
        return Ok(None);
    }
    let row: Option<Upload> = diesel::delete(uploads::table.filter(uploads::url.eq(url))).returning(Upload::as_returning()).get_result(conn).optional()?;
    Ok(row.map(|u| (u.proveedor, u.clave, serde_json::from_value(u.variantes).unwrap_or_default())))
}
