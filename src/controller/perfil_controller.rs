use crate::auth::AuthUser;
use crate::db::{self, DbPool};
use crate::errors::ApiResult;
use crate::models::enum_role::Role;
use crate::models::perfil::PerfilUpdate;
use crate::services::common::Pagination;
use crate::services::perfil_services::{self as svc, CreatePerfilRequest};
use actix_web::{web, HttpResponse};
use uuid::Uuid;

pub async fn create(user: AuthUser, body: web::Json<CreatePerfilRequest>, pool: web::Data<DbPool>) -> ApiResult<HttpResponse> {
    Ok(HttpResponse::Created().json(db::run(&pool, move |conn| svc::create(conn, user.id, body.into_inner())).await?))
}

pub async fn mine(user: AuthUser, pool: web::Data<DbPool>) -> ApiResult<HttpResponse> {
    Ok(HttpResponse::Ok().json(db::run(&pool, move |conn| svc::get_by_user(conn, user.id)).await?))
}

pub async fn update_mine(user: AuthUser, body: web::Json<PerfilUpdate>, pool: web::Data<DbPool>) -> ApiResult<HttpResponse> {
    Ok(HttpResponse::Ok().json(db::run(&pool, move |conn| svc::update(conn, user.id, body.into_inner())).await?))
}

pub async fn admin_get(path: web::Path<Uuid>, user: AuthUser, pool: web::Data<DbPool>) -> ApiResult<HttpResponse> {
    user.require(&[Role::Admin])?;
    let user_id = path.into_inner();
    Ok(HttpResponse::Ok().json(db::run(&pool, move |conn| svc::get_by_user(conn, user_id)).await?))
}

pub async fn admin_list(user: AuthUser, page: web::Query<Pagination>, pool: web::Data<DbPool>) -> ApiResult<HttpResponse> {
    user.require(&[Role::Admin])?;
    let p = page.into_inner();
    Ok(HttpResponse::Ok().json(db::run(&pool, move |conn| svc::list(conn, p)).await?))
}
