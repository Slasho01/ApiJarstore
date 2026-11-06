use super::ok_message;
use crate::auth::AuthUser;
use crate::db::{self, DbPool};
use crate::errors::{ApiError, ApiResult};
use crate::models::enum_role::Role;
use crate::models::users::UserResponse;
use crate::services::common::Pagination;
use crate::services::user_services::{self, ChangePasswordRequest, UserFilter};
use actix_web::{web, HttpResponse};
use serde::Deserialize;
use uuid::Uuid;

#[derive(Deserialize)]
pub struct CodigoBody {
    pub codigo: String,
}

/// Genera el secreto 2FA (sin activar) y devuelve QR + secreto.
pub async fn twofa_setup(user: AuthUser, pool: web::Data<DbPool>) -> ApiResult<HttpResponse> {
    let id = user.id;
    let (secret, url, qr) = db::run(&pool, move |conn| user_services::twofa_setup(conn, id)).await?;
    Ok(HttpResponse::Ok().json(serde_json::json!({ "secret": secret, "url": url, "qr": qr })))
}

/// Activa el 2FA tras verificar un código.
pub async fn twofa_enable(user: AuthUser, body: web::Json<CodigoBody>, pool: web::Data<DbPool>) -> ApiResult<HttpResponse> {
    let (id, codigo) = (user.id, body.into_inner().codigo);
    db::run(&pool, move |conn| user_services::twofa_enable(conn, id, &codigo)).await?;
    Ok(ok_message("2FA activado"))
}

/// Desactiva el 2FA.
pub async fn twofa_disable(user: AuthUser, body: web::Json<CodigoBody>, pool: web::Data<DbPool>) -> ApiResult<HttpResponse> {
    let (id, codigo) = (user.id, body.into_inner().codigo);
    db::run(&pool, move |conn| user_services::twofa_disable(conn, id, &codigo)).await?;
    Ok(ok_message("2FA desactivado"))
}

/// Cierra la sesión en todos los dispositivos (invalida todos los tokens).
pub async fn logout_all(user: AuthUser, pool: web::Data<DbPool>) -> ApiResult<HttpResponse> {
    let id = user.id;
    db::run(&pool, move |conn| user_services::logout_all(conn, id)).await?;
    Ok(ok_message("Se cerraron todas las sesiones"))
}

pub async fn me(user: AuthUser, pool: web::Data<DbPool>) -> ApiResult<HttpResponse> {
    let u = db::run(&pool, move |conn| user_services::get_user(conn, user.id)).await?;
    Ok(HttpResponse::Ok().json(UserResponse::from(u)))
}

pub async fn get_by_id(path: web::Path<Uuid>, user: AuthUser, pool: web::Data<DbPool>) -> ApiResult<HttpResponse> {
    let id = path.into_inner();
    if id != user.id && !user.is_admin() {
        return Err(ApiError::forbidden("No tienes permiso para acceder a este recurso"));
    }
    let u = db::run(&pool, move |conn| user_services::get_user(conn, id)).await?;
    Ok(HttpResponse::Ok().json(UserResponse::from(u)))
}

pub async fn change_my_password(
    user: AuthUser,
    body: web::Json<ChangePasswordRequest>,
    pool: web::Data<DbPool>,
) -> ApiResult<HttpResponse> {
    db::run(&pool, move |conn| user_services::change_password(conn, user.id, body.into_inner())).await?;
    Ok(ok_message("¡Contraseña actualizada exitosamente!"))
}

/// Ruta antigua `PUT /api/users/{id}`: sólo sobre uno mismo.
pub async fn change_password_by_id(
    path: web::Path<Uuid>,
    user: AuthUser,
    body: web::Json<ChangePasswordRequest>,
    pool: web::Data<DbPool>,
) -> ApiResult<HttpResponse> {
    if path.into_inner() != user.id {
        return Err(ApiError::forbidden("Sólo puedes cambiar tu propia contraseña"));
    }
    change_my_password(user, body, pool).await
}

// ---------- Admin ----------

pub async fn admin_list(
    user: AuthUser,
    filter: web::Query<UserFilter>,
    page: web::Query<Pagination>,
    pool: web::Data<DbPool>,
) -> ApiResult<HttpResponse> {
    user.require(&[Role::Admin])?;
    let (f, p) = (filter.into_inner(), page.into_inner());
    Ok(HttpResponse::Ok().json(db::run(&pool, move |conn| user_services::list_users(conn, f, p)).await?))
}

#[derive(Deserialize)]
pub struct RoleRequest {
    pub role: Role,
}

pub async fn admin_set_role(
    path: web::Path<Uuid>,
    user: AuthUser,
    body: web::Json<RoleRequest>,
    pool: web::Data<DbPool>,
) -> ApiResult<HttpResponse> {
    user.require(&[Role::Admin])?;
    let id = path.into_inner();
    if id == user.id {
        return Err(ApiError::bad_request("No puedes cambiar tu propio rol"));
    }
    let role = body.role;
    Ok(HttpResponse::Ok().json(db::run(&pool, move |conn| user_services::set_role(conn, id, role)).await?))
}

pub async fn admin_delete(path: web::Path<Uuid>, user: AuthUser, pool: web::Data<DbPool>) -> ApiResult<HttpResponse> {
    user.require(&[Role::Admin])?;
    let id = path.into_inner();
    if id == user.id {
        return Err(ApiError::bad_request("No puedes eliminar tu propia cuenta desde aquí"));
    }
    db::run(&pool, move |conn| user_services::delete_user(conn, id)).await?;
    Ok(HttpResponse::NoContent().finish())
}
