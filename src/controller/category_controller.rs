use crate::auth::AuthUser;
use crate::db::{self, DbPool};
use crate::errors::ApiResult;
use crate::models::category::NewCategoria;
use crate::services::category_services::CategoriaUpdateRequest;
use crate::models::enum_role::Role;
use crate::services::category_services as svc;
use actix_web::{web, HttpResponse};
use uuid::Uuid;

/// Público: sólo activas. Un admin ve también las inactivas.
pub async fn list(user: Option<AuthUser>, pool: web::Data<DbPool>) -> ApiResult<HttpResponse> {
    let all = user.map(|u| u.is_admin()).unwrap_or(false);
    Ok(HttpResponse::Ok().json(db::run(&pool, move |conn| svc::list(conn, all)).await?))
}

/// Por id o slug. Incluye la ruta de ancestros para breadcrumbs.
pub async fn get(path: web::Path<String>, pool: web::Data<DbPool>) -> ApiResult<HttpResponse> {
    let key = path.into_inner();
    Ok(HttpResponse::Ok().json(db::run(&pool, move |conn| {
        let cat = svc::get(conn, &key)?;
        let ruta = svc::ancestors(conn, &cat)?;
        Ok(serde_json::json!({ "categoria": cat, "ruta": ruta }))
    }).await?))
}

pub async fn create(user: AuthUser, body: web::Json<NewCategoria>, pool: web::Data<DbPool>) -> ApiResult<HttpResponse> {
    user.require(&[Role::Admin])?;
    Ok(HttpResponse::Created().json(db::run(&pool, move |conn| svc::create(conn, body.into_inner())).await?))
}

pub async fn update(
    path: web::Path<Uuid>,
    user: AuthUser,
    body: web::Json<CategoriaUpdateRequest>,
    pool: web::Data<DbPool>,
) -> ApiResult<HttpResponse> {
    user.require(&[Role::Admin])?;
    let id = path.into_inner();
    Ok(HttpResponse::Ok().json(db::run(&pool, move |conn| svc::update(conn, id, body.into_inner())).await?))
}

pub async fn delete(path: web::Path<Uuid>, user: AuthUser, pool: web::Data<DbPool>) -> ApiResult<HttpResponse> {
    user.require(&[Role::Admin])?;
    let id = path.into_inner();
    db::run(&pool, move |conn| svc::delete(conn, id)).await?;
    Ok(HttpResponse::NoContent().finish())
}
