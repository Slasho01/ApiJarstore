use crate::auth::AuthUser;
use crate::config::Config;
use crate::controller::auth_controller::html_escape;
use crate::controller::upload_controller::release;
use crate::db::{self, DbPool};
use crate::errors::ApiResult;
use crate::mailer::Mailer;
use crate::models::enum_role::Role;
use crate::models::tienda::BannerData;
use crate::services::common::Pagination;
use crate::services::marketing_services::{self as svc, Alta};
use crate::storage::Storage;
use actix_web::{web, HttpResponse};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

#[derive(Deserialize)]
pub struct BannerQuery {
    pub ubicacion: Option<String>,
}

pub async fn banners(q: web::Query<BannerQuery>, pool: web::Data<DbPool>) -> ApiResult<HttpResponse> {
    let u = q.into_inner().ubicacion;
    Ok(HttpResponse::Ok()
        .insert_header(("Cache-Control", "public, max-age=60"))
        .json(db::run(&pool, move |conn| svc::active_banners(conn, u.as_deref())).await?))
}

pub async fn admin_banners(user: AuthUser, pool: web::Data<DbPool>) -> ApiResult<HttpResponse> {
    user.require(&[Role::Admin])?;
    Ok(HttpResponse::Ok().json(db::run(&pool, svc::all_banners).await?))
}

pub async fn admin_banner_create(user: AuthUser, body: web::Json<BannerData>, pool: web::Data<DbPool>) -> ApiResult<HttpResponse> {
    user.require(&[Role::Admin])?;
    Ok(HttpResponse::Created().json(db::run(&pool, move |conn| svc::create_banner(conn, body.into_inner())).await?))
}

pub async fn admin_banner_update(path: web::Path<Uuid>, user: AuthUser, body: web::Json<BannerData>, pool: web::Data<DbPool>) -> ApiResult<HttpResponse> {
    user.require(&[Role::Admin])?;
    let id = path.into_inner();
    Ok(HttpResponse::Ok().json(db::run(&pool, move |conn| svc::update_banner(conn, id, body.into_inner())).await?))
}

pub async fn admin_banner_delete(path: web::Path<Uuid>, user: AuthUser, pool: web::Data<DbPool>, storage: web::Data<Storage>) -> ApiResult<HttpResponse> {
    user.require(&[Role::Admin])?;
    let id = path.into_inner();
    let b = db::run(&pool, move |conn| svc::delete_banner(conn, id)).await?;
    for url in [b.imagen_url, b.imagen_movil_url].into_iter().flatten() {
        release(&pool, &storage, url).await;
    }
    Ok(HttpResponse::NoContent().finish())
}

// ---------- Newsletter ----------

#[derive(Deserialize)]
pub struct EmailBody {
    pub email: String,
}

#[derive(Deserialize)]
pub struct TokenBody {
    pub token: String,
}

pub async fn subscribe(
    body: web::Json<EmailBody>,
    pool: web::Data<DbPool>,
    mailer: web::Data<Mailer>,
    config: web::Data<Config>,
) -> ApiResult<HttpResponse> {
    let email = body.into_inner().email;
    let e2 = email.clone();
    let alta = db::run(&pool, move |conn| svc::subscribe(conn, &e2)).await?;
    if let Alta::Confirmar(token) = alta {
        let tienda = db::run(&pool, crate::services::tienda_services::tienda).await.map(|t| t.nombre).unwrap_or_else(|_| "JarStore".into());
        let link = format!("{}/newsletter/confirmar?token={token}", config.frontend_url);
        let baja = format!("{}/newsletter/baja?token={token}", config.frontend_url);
        let text = format!("Confirma tu suscripción a las novedades de {tienda}:\n{link}\n\nSi no fuiste tú, ignora este correo.\nDarte de baja: {baja}");
        let html = format!("<p>Confirma tu suscripción a las novedades de <b>{}</b>:</p><p><a href=\"{link}\">Confirmar suscripción</a></p><p style=\"color:#666;font-size:12px\">Si no fuiste tú, ignora este correo. <a href=\"{baja}\">Darme de baja</a></p>", html_escape(&tienda));
        let mailer = mailer.clone();
        let to = email.trim().to_lowercase();
        actix_web::rt::task::spawn_blocking(move || {
            if let Err(e) = mailer.send(&to, "Confirma tu suscripción", &text, &html) {
                log::error!("No se pudo enviar la confirmación de newsletter: {e}");
            }
        });
    }
    // Misma respuesta en todos los casos (no revela si el email ya estaba)
    Ok(HttpResponse::Ok().json(json!({ "message": "Revisa tu correo para confirmar la suscripción" })))
}

pub async fn confirm(body: web::Json<TokenBody>, pool: web::Data<DbPool>) -> ApiResult<HttpResponse> {
    let t = body.into_inner().token;
    let email = db::run(&pool, move |conn| svc::confirm(conn, &t)).await?;
    Ok(HttpResponse::Ok().json(json!({ "message": "¡Suscripción confirmada!", "email": email })))
}

pub async fn unsubscribe(body: web::Json<TokenBody>, pool: web::Data<DbPool>) -> ApiResult<HttpResponse> {
    let t = body.into_inner().token;
    let email = db::run(&pool, move |conn| svc::unsubscribe(conn, &t)).await?;
    Ok(HttpResponse::Ok().json(json!({ "message": "Te diste de baja de las novedades", "email": email })))
}

#[derive(Deserialize)]
pub struct SubsQuery {
    pub confirmados: Option<bool>,
}

pub async fn admin_subscribers(user: AuthUser, page: web::Query<Pagination>, q: web::Query<SubsQuery>, pool: web::Data<DbPool>) -> ApiResult<HttpResponse> {
    user.require(&[Role::Admin])?;
    let (p, c) = (page.into_inner(), q.confirmados.unwrap_or(false));
    Ok(HttpResponse::Ok().json(db::run(&pool, move |conn| svc::subscribers(conn, p, c)).await?))
}

pub async fn admin_subscribers_csv(user: AuthUser, pool: web::Data<DbPool>) -> ApiResult<HttpResponse> {
    user.require(&[Role::Admin])?;
    let csv = db::run(&pool, svc::export_csv).await?;
    Ok(HttpResponse::Ok()
        .content_type("text/csv; charset=utf-8")
        .insert_header(("Content-Disposition", "attachment; filename=\"newsletter.csv\""))
        .body(csv))
}
