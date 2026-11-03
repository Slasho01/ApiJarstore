use crate::auth::AuthUser;
use crate::db::{self, DbPool};
use crate::errors::ApiResult;
use crate::models::enum_role::Role;
use crate::models::extras::{CuponUpdate, DireccionData, NewCupon, ResenaUpdate};
use crate::services::common::Pagination;
use crate::services::extras_services::{self as svc, ResenaRequest, WishlistRequest};
use actix_web::{web, HttpResponse};
use uuid::Uuid;

// ---------- Reseñas ----------

pub async fn reviews_list(path: web::Path<Uuid>, page: web::Query<Pagination>, pool: web::Data<DbPool>) -> ApiResult<HttpResponse> {
    let (id, p) = (path.into_inner(), page.into_inner());
    Ok(HttpResponse::Ok().json(db::run(&pool, move |conn| svc::list_reviews(conn, id, p)).await?))
}

pub async fn review_create(
    path: web::Path<Uuid>,
    user: AuthUser,
    body: web::Json<ResenaRequest>,
    pool: web::Data<DbPool>,
) -> ApiResult<HttpResponse> {
    let id = path.into_inner();
    Ok(HttpResponse::Created().json(db::run(&pool, move |conn| svc::create_review(conn, id, user, body.into_inner())).await?))
}

pub async fn review_update(
    path: web::Path<Uuid>,
    user: AuthUser,
    body: web::Json<ResenaUpdate>,
    pool: web::Data<DbPool>,
) -> ApiResult<HttpResponse> {
    let id = path.into_inner();
    Ok(HttpResponse::Ok().json(db::run(&pool, move |conn| svc::update_review(conn, id, user, body.into_inner())).await?))
}

pub async fn review_delete(path: web::Path<Uuid>, user: AuthUser, pool: web::Data<DbPool>) -> ApiResult<HttpResponse> {
    let id = path.into_inner();
    db::run(&pool, move |conn| svc::delete_review(conn, id, user)).await?;
    Ok(HttpResponse::NoContent().finish())
}

// ---------- Wishlist ----------

pub async fn wishlist(user: AuthUser, pool: web::Data<DbPool>) -> ApiResult<HttpResponse> {
    Ok(HttpResponse::Ok().json(db::run(&pool, move |conn| svc::wishlist_of(conn, user.id)).await?))
}

pub async fn wishlist_add(user: AuthUser, body: web::Json<WishlistRequest>, pool: web::Data<DbPool>) -> ApiResult<HttpResponse> {
    let pid = body.producto_id;
    Ok(HttpResponse::Ok().json(db::run(&pool, move |conn| svc::wishlist_add(conn, user.id, pid)).await?))
}

pub async fn wishlist_remove(path: web::Path<Uuid>, user: AuthUser, pool: web::Data<DbPool>) -> ApiResult<HttpResponse> {
    let pid = path.into_inner();
    db::run(&pool, move |conn| svc::wishlist_remove(conn, user.id, pid)).await?;
    Ok(HttpResponse::NoContent().finish())
}

// ---------- Direcciones ----------

pub async fn addresses(user: AuthUser, pool: web::Data<DbPool>) -> ApiResult<HttpResponse> {
    Ok(HttpResponse::Ok().json(db::run(&pool, move |conn| svc::addresses_of(conn, user.id)).await?))
}

pub async fn address_create(user: AuthUser, body: web::Json<DireccionData>, pool: web::Data<DbPool>) -> ApiResult<HttpResponse> {
    Ok(HttpResponse::Created().json(db::run(&pool, move |conn| svc::address_create(conn, user.id, body.into_inner())).await?))
}

pub async fn address_update(
    path: web::Path<Uuid>,
    user: AuthUser,
    body: web::Json<DireccionData>,
    pool: web::Data<DbPool>,
) -> ApiResult<HttpResponse> {
    let id = path.into_inner();
    Ok(HttpResponse::Ok().json(db::run(&pool, move |conn| svc::address_update(conn, user.id, id, body.into_inner())).await?))
}

pub async fn address_delete(path: web::Path<Uuid>, user: AuthUser, pool: web::Data<DbPool>) -> ApiResult<HttpResponse> {
    let id = path.into_inner();
    db::run(&pool, move |conn| svc::address_delete(conn, user.id, id)).await?;
    Ok(HttpResponse::NoContent().finish())
}

// ---------- Cupones ----------

pub async fn coupon_check(path: web::Path<String>, user: AuthUser, pool: web::Data<DbPool>) -> ApiResult<HttpResponse> {
    let codigo = path.into_inner();
    Ok(HttpResponse::Ok().json(db::run(&pool, move |conn| svc::coupon_check(conn, user.id, &codigo)).await?))
}

pub async fn coupons_list(user: AuthUser, page: web::Query<Pagination>, pool: web::Data<DbPool>) -> ApiResult<HttpResponse> {
    user.require(&[Role::Admin])?;
    let p = page.into_inner();
    Ok(HttpResponse::Ok().json(db::run(&pool, move |conn| svc::coupon_list(conn, p)).await?))
}

pub async fn coupon_create(user: AuthUser, body: web::Json<NewCupon>, pool: web::Data<DbPool>) -> ApiResult<HttpResponse> {
    user.require(&[Role::Admin])?;
    Ok(HttpResponse::Created().json(db::run(&pool, move |conn| svc::coupon_create(conn, body.into_inner())).await?))
}

pub async fn coupon_update(
    path: web::Path<Uuid>,
    user: AuthUser,
    body: web::Json<CuponUpdate>,
    pool: web::Data<DbPool>,
) -> ApiResult<HttpResponse> {
    user.require(&[Role::Admin])?;
    let id = path.into_inner();
    Ok(HttpResponse::Ok().json(db::run(&pool, move |conn| svc::coupon_update(conn, id, body.into_inner())).await?))
}

pub async fn coupon_delete(path: web::Path<Uuid>, user: AuthUser, pool: web::Data<DbPool>) -> ApiResult<HttpResponse> {
    user.require(&[Role::Admin])?;
    let id = path.into_inner();
    db::run(&pool, move |conn| svc::coupon_delete(conn, id)).await?;
    Ok(HttpResponse::NoContent().finish())
}
