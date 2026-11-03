use crate::auth::AuthUser;
use crate::db::{self, DbPool};
use crate::errors::ApiResult;
use crate::models::enum_role::Role;
use crate::services::common::Pagination;
use crate::services::puntos_services as svc;
use actix_web::{web, HttpResponse};
use serde::Deserialize;
use uuid::Uuid;

/// GET /api/puntos — saldo, equivalencia, por acreditar e historial del usuario.
pub async fn mine(user: AuthUser, page: web::Query<Pagination>, pool: web::Data<DbPool>) -> ApiResult<HttpResponse> {
    let p = page.into_inner();
    Ok(HttpResponse::Ok().json(db::run(&pool, move |conn| svc::resumen(conn, user.id, p)).await?))
}

pub async fn admin_get(path: web::Path<Uuid>, user: AuthUser, page: web::Query<Pagination>, pool: web::Data<DbPool>) -> ApiResult<HttpResponse> {
    user.require(&[Role::Admin])?;
    let (id, p) = (path.into_inner(), page.into_inner());
    Ok(HttpResponse::Ok().json(db::run(&pool, move |conn| svc::resumen(conn, id, p)).await?))
}

#[derive(Deserialize)]
pub struct AjusteRequest {
    pub puntos: i64,
    pub motivo: String,
}

/// Ajuste manual (positivo o negativo) con motivo, queda en el historial.
pub async fn admin_adjust(path: web::Path<Uuid>, user: AuthUser, body: web::Json<AjusteRequest>, pool: web::Data<DbPool>) -> ApiResult<HttpResponse> {
    user.require(&[Role::Admin])?;
    let id = path.into_inner();
    let AjusteRequest { puntos, motivo } = body.into_inner();
    let saldo = db::run(&pool, move |conn| svc::adjust(conn, id, puntos, &motivo)).await?;
    Ok(HttpResponse::Ok().json(serde_json::json!({ "saldo": saldo })))
}
