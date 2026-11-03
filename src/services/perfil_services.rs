use crate::errors::{ApiError, ApiResult, OrNotFound};
use crate::models::perfil::{NewPerfil, Perfil, PerfilUpdate};
use crate::schema::perfil;
use crate::services::common::{now, optional_text, required_text, Paginated, Pagination};
use diesel::prelude::*;
use serde::Deserialize;
use uuid::Uuid;

#[derive(Deserialize)]
pub struct CreatePerfilRequest {
    pub nombre: String,
    pub apellido: String,
    pub telefono: Option<String>,
    pub direccion: Option<String>,
}

pub fn create(conn: &mut PgConnection, user_id: Uuid, req: CreatePerfilRequest) -> ApiResult<Perfil> {
    diesel::insert_into(perfil::table)
        .values(NewPerfil {
            user_id: Some(user_id), // siempre el del token, nunca del body
            nombre: required_text("nombre", &req.nombre, 255)?,
            apellido: required_text("apellido", &req.apellido, 255)?,
            telefono: optional_text("telefono", req.telefono, 255)?,
            direccion: optional_text("direccion", req.direccion, 2000)?,
        })
        .returning(Perfil::as_returning())
        .get_result(conn)
        .map_err(|e| match ApiError::from(e) {
            ApiError::Conflict(_) => ApiError::conflict("Este usuario ya tiene un perfil registrado"),
            other => other,
        })
}

pub fn get_by_user(conn: &mut PgConnection, user_id: Uuid) -> ApiResult<Perfil> {
    perfil::table
        .filter(perfil::user_id.eq(user_id))
        .select(Perfil::as_select())
        .first(conn)
        .or_not_found("Perfil")
}

pub fn list(conn: &mut PgConnection, p: Pagination) -> ApiResult<Paginated<Perfil>> {
    let total: i64 = perfil::table.count().get_result(conn)?;
    let items = perfil::table
        .order(perfil::created_at.desc())
        .limit(p.per_page())
        .offset(p.offset())
        .select(Perfil::as_select())
        .load(conn)?;
    Ok(Paginated::new(items, &p, total))
}

pub fn update(conn: &mut PgConnection, user_id: Uuid, mut data: PerfilUpdate) -> ApiResult<Perfil> {
    if let Some(n) = &data.nombre {
        data.nombre = Some(required_text("nombre", n, 255)?);
    }
    if let Some(a) = &data.apellido {
        data.apellido = Some(required_text("apellido", a, 255)?);
    }
    data.updated_at = Some(now());
    diesel::update(perfil::table.filter(perfil::user_id.eq(user_id)))
        .set(&data)
        .returning(Perfil::as_returning())
        .get_result(conn)
        .or_not_found("Perfil")
}
