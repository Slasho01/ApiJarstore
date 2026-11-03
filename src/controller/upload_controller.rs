use crate::auth::AuthUser;
use crate::db::{self, DbPool};
use crate::errors::{ApiError, ApiResult};
use crate::models::enum_role::Role;
use crate::services::upload_services;
use crate::storage::{Storage, MAX_BYTES};
use actix_multipart::Multipart;
use actix_web::{web, HttpResponse};
use futures_util::StreamExt;

/// POST /api/uploads (multipart): campo `file` y opcional `nombre` (para el nombre del archivo).
pub async fn upload(user: AuthUser, mut payload: Multipart, pool: web::Data<DbPool>, storage: web::Data<Storage>) -> ApiResult<HttpResponse> {
    user.require(&[Role::Admin, Role::Seller])?;
    let mut file: Option<Vec<u8>> = None;
    let mut nombre = String::from("imagen");
    while let Some(field) = payload.next().await {
        let mut field = field.map_err(|e| ApiError::bad_request(format!("Formulario inválido: {e}")))?;
        let name = field.name().unwrap_or_default().to_string();
        let mut buf = Vec::new();
        while let Some(chunk) = field.next().await {
            let chunk = chunk.map_err(|e| ApiError::bad_request(format!("Error leyendo el archivo: {e}")))?;
            if buf.len() + chunk.len() > MAX_BYTES {
                return Err(ApiError::bad_request("La imagen supera los 10 MB"));
            }
            buf.extend_from_slice(&chunk);
        }
        match name.as_str() {
            "file" => {
                if nombre == "imagen" {
                    if let Some(fname) = field.content_disposition().and_then(|cd| cd.get_filename()) {
                        nombre = fname.rsplit_once('.').map(|(n, _)| n).unwrap_or(fname).to_string();
                    }
                }
                file = Some(buf)
            }
            "nombre" => nombre = String::from_utf8_lossy(&buf).trim().chars().take(100).collect(),
            _ => {}
        }
    }
    let data = file.ok_or_else(|| ApiError::bad_request("Falta el archivo (campo 'file')"))?;
    let stored = storage.store(data, &nombre).await?;
    let user_id = user.id;
    let saved = db::run(&pool, move |conn| upload_services::record(conn, user_id, &stored)).await?;
    Ok(HttpResponse::Created().json(saved))
}

/// Borra del almacenamiento una imagen que ya no se usa (en segundo plano).
pub async fn release(pool: &web::Data<DbPool>, storage: &web::Data<Storage>, url: String) {
    match db::run(pool, move |conn| upload_services::release(conn, &url)).await {
        Ok(Some((proveedor, clave, variantes))) => storage.delete(&proveedor, &clave, &variantes).await,
        Ok(None) => {}
        Err(e) => log::warn!("No se pudo liberar la imagen: {e}"),
    }
}
