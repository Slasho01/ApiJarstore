use crate::auth::AuthUser;
use crate::db::{self, DbPool};
use crate::errors::ApiResult;
use crate::melihub::Melihub;
use crate::models::enum_role::Role;
use crate::schema::productos;
use crate::services::melihub_services as mh;
use actix_web::{web, HttpResponse};
use diesel::prelude::*;
use serde_json::json;

/// Importa a JarStore las publicaciones de Mercado Libre centralizadas en MeliHub
/// (crea/actualiza productos). Sólo admin.
pub async fn admin_sync(user: AuthUser, pool: web::Data<DbPool>, melihub: web::Data<Melihub>) -> ApiResult<HttpResponse> {
    user.require(&[Role::Admin])?;
    let client = melihub.get_ref().clone();
    let owner = user.id;
    let res = db::run(&pool, move |conn| mh::importar(conn, &client, owner)).await?;
    Ok(HttpResponse::Ok().json(res))
}

/// Estado de la integración: si está configurada y cuántos productos están vinculados.
pub async fn admin_status(user: AuthUser, pool: web::Data<DbPool>, melihub: web::Data<Melihub>) -> ApiResult<HttpResponse> {
    user.require(&[Role::Admin])?;
    let configurado = melihub.enabled();
    let vinculados: i64 = db::run(&pool, move |conn| {
        productos::table
            .filter(productos::melihub_id.is_not_null())
            .count()
            .get_result(conn)
            .map_err(Into::into)
    })
    .await?;
    Ok(HttpResponse::Ok().json(json!({ "configurado": configurado, "vinculados": vinculados })))
}
