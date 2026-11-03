use crate::auth::AuthUser;
use crate::db::{self, DbPool};
use crate::errors::ApiResult;
use crate::mailer::Mailer;
use crate::models::enum_role::Role;
use crate::models::tienda::{NewPagina, PaginaUpdate};
use crate::services::tienda_services as svc;
use actix_web::{web, HttpResponse};
use serde_json::json;
use uuid::Uuid;

/// Configuración pública (tienda, monedas activas, envíos, pagos, páginas visibles).
pub async fn public_config(pool: web::Data<DbPool>, pay: web::Data<crate::controller::order_controller::Payments>) -> ApiResult<HttpResponse> {
    let mp = pay.mp.is_some();
    Ok(HttpResponse::Ok()
        .insert_header(("Cache-Control", "public, max-age=60"))
        .json(db::run(&pool, move |conn| svc::public_config(conn, mp)).await?))
}

pub async fn page(path: web::Path<String>, user: Option<AuthUser>, pool: web::Data<DbPool>) -> ApiResult<HttpResponse> {
    let slug = path.into_inner();
    let admin = user.map(|u| u.is_admin()).unwrap_or(false);
    Ok(HttpResponse::Ok().json(db::run(&pool, move |conn| svc::page_by_slug(conn, &slug, admin)).await?))
}

// ---------- Admin ----------

pub async fn admin_config(
    user: AuthUser,
    pool: web::Data<DbPool>,
    mailer: web::Data<Mailer>,
    pay: web::Data<crate::controller::order_controller::Payments>,
    storage: web::Data<crate::storage::Storage>,
    courier: web::Data<crate::courier::Courier>,
) -> ApiResult<HttpResponse> {
    user.require(&[Role::Admin])?;
    let m = mailer.get_ref().clone();
    let (entries, smtp_ok) = db::run(&pool, move |conn| {
        let entries = svc::admin_config(conn)?;
        let smtp_ok = m.is_configured(conn);
        Ok((entries, smtp_ok))
    }).await?;
    Ok(HttpResponse::Ok().json(json!({
        "config": entries,
        "smtp_configurado": smtp_ok,
        "mercadopago_configurado": pay.mp.is_some(),
        "bluexpress_configurado": courier.enabled(),
        "almacenamiento_imagenes": storage.provider(),
    })))
}

#[derive(serde::Deserialize)]
pub struct SmtpTestRequest {
    pub email: Option<String>,
}

/// Envía un correo de prueba con la configuración SMTP actual (reporta el error real).
pub async fn admin_smtp_test(
    user: AuthUser,
    body: web::Json<SmtpTestRequest>,
    pool: web::Data<DbPool>,
    mailer: web::Data<Mailer>,
) -> ApiResult<HttpResponse> {
    user.require(&[Role::Admin])?;
    let dest = match body.into_inner().email.filter(|e| e.contains('@')) {
        Some(e) => e,
        None => {
            let uid = user.id;
            db::run(&pool, move |conn| crate::services::user_services::get_user(conn, uid)).await?.email
        }
    };
    let m = mailer.get_ref().clone();
    let to = dest.clone();
    let res = actix_web::web::block(move || {
        m.send_strict(&to, "Correo de prueba — JarStore",
            "Este es un correo de prueba de JarStore. Si lo recibiste, tu configuración SMTP funciona.",
            "<p>Este es un <b>correo de prueba</b> de JarStore.</p><p>Si lo recibiste, tu configuración SMTP funciona correctamente. 🎉</p>")
    }).await.map_err(|e| crate::errors::ApiError::internal(e.to_string()))?;
    match res {
        Ok(true) => Ok(HttpResponse::Ok().json(json!({ "message": format!("Correo de prueba enviado a {dest}") }))),
        Ok(false) => Err(crate::errors::ApiError::bad_request("No hay SMTP configurado. Activa y completa el servidor de correo, o define las variables SMTP en el servidor.")),
        Err(e) => Err(crate::errors::ApiError::bad_request(format!("No se pudo enviar: {e}"))),
    }
}

pub async fn admin_set_config(
    path: web::Path<String>,
    user: AuthUser,
    body: web::Json<serde_json::Value>,
    pool: web::Data<DbPool>,
) -> ApiResult<HttpResponse> {
    user.require(&[Role::Admin])?;
    let clave = path.into_inner();
    let valor = body.into_inner();
    Ok(HttpResponse::Ok().json(db::run(&pool, move |conn| svc::set_config(conn, &clave, valor)).await?))
}

pub async fn admin_metrics(user: AuthUser, q: web::Query<crate::services::metrics_services::RangoQuery>, pool: web::Data<DbPool>) -> ApiResult<HttpResponse> {
    user.require(&[Role::Admin])?;
    let q = q.into_inner();
    Ok(HttpResponse::Ok().json(db::run(&pool, move |conn| crate::services::metrics_services::metrics(conn, q)).await?))
}

pub async fn admin_pages(user: AuthUser, pool: web::Data<DbPool>) -> ApiResult<HttpResponse> {
    user.require(&[Role::Admin])?;
    Ok(HttpResponse::Ok().json(db::run(&pool, svc::pages_all).await?))
}

pub async fn admin_page_create(user: AuthUser, body: web::Json<NewPagina>, pool: web::Data<DbPool>) -> ApiResult<HttpResponse> {
    user.require(&[Role::Admin])?;
    Ok(HttpResponse::Created().json(db::run(&pool, move |conn| svc::page_create(conn, body.into_inner())).await?))
}

pub async fn admin_page_update(
    path: web::Path<Uuid>,
    user: AuthUser,
    body: web::Json<PaginaUpdate>,
    pool: web::Data<DbPool>,
) -> ApiResult<HttpResponse> {
    user.require(&[Role::Admin])?;
    let id = path.into_inner();
    Ok(HttpResponse::Ok().json(db::run(&pool, move |conn| svc::page_update(conn, id, body.into_inner())).await?))
}

pub async fn admin_page_delete(path: web::Path<Uuid>, user: AuthUser, pool: web::Data<DbPool>) -> ApiResult<HttpResponse> {
    user.require(&[Role::Admin])?;
    let id = path.into_inner();
    db::run(&pool, move |conn| svc::page_delete(conn, id)).await?;
    Ok(HttpResponse::NoContent().finish())
}
