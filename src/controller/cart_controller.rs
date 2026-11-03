use crate::auth::AuthUser;
use crate::db::{self, DbPool};
use crate::errors::ApiResult;
use crate::services::cart_services::{self as svc, AddToCartRequest, UpdateQuantityRequest};
use actix_web::{web, HttpResponse};
use uuid::Uuid;

pub async fn view(user: AuthUser, pool: web::Data<DbPool>) -> ApiResult<HttpResponse> {
    Ok(HttpResponse::Ok().json(db::run(&pool, move |conn| svc::view(conn, user.id)).await?))
}

pub async fn add(user: AuthUser, body: web::Json<AddToCartRequest>, pool: web::Data<DbPool>) -> ApiResult<HttpResponse> {
    Ok(HttpResponse::Ok().json(db::run(&pool, move |conn| svc::add(conn, user.id, body.into_inner())).await?))
}

pub async fn update_quantity(
    path: web::Path<Uuid>,
    user: AuthUser,
    body: web::Json<UpdateQuantityRequest>,
    pool: web::Data<DbPool>,
) -> ApiResult<HttpResponse> {
    let item_id = path.into_inner();
    let qty = body.cantidad;
    Ok(HttpResponse::Ok().json(db::run(&pool, move |conn| svc::update_quantity(conn, user.id, item_id, qty)).await?))
}

pub async fn remove(path: web::Path<Uuid>, user: AuthUser, pool: web::Data<DbPool>) -> ApiResult<HttpResponse> {
    let item_id = path.into_inner();
    db::run(&pool, move |conn| svc::remove(conn, user.id, item_id)).await?;
    Ok(HttpResponse::NoContent().finish())
}

pub async fn clear(user: AuthUser, pool: web::Data<DbPool>) -> ApiResult<HttpResponse> {
    db::run(&pool, move |conn| svc::clear(conn, user.id)).await?;
    Ok(HttpResponse::NoContent().finish())
}
