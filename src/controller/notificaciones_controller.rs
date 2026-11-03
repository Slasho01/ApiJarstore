use crate::auth::AuthUser;
use crate::db::{self, DbPool};
use crate::errors::ApiResult;
use crate::mailer::Mailer;
use crate::models::enum_role::Role;
use crate::services::notificaciones_services as noti;
use actix_web::{web, HttpResponse};
use serde::Deserialize;
use uuid::Uuid;

#[derive(Deserialize)]
pub struct AvisoStockRequest {
    pub email: String,
    pub variante_id: Option<Uuid>,
}

/// "Avísame cuando vuelva": suscribe un correo a la reposición de un producto agotado.
pub async fn subscribe_stock(
    path: web::Path<Uuid>,
    user: Option<AuthUser>,
    body: web::Json<AvisoStockRequest>,
    pool: web::Data<DbPool>,
) -> ApiResult<HttpResponse> {
    let pid = path.into_inner();
    let AvisoStockRequest { email, variante_id } = body.into_inner();
    let uid = user.map(|u| u.id);
    db::run(&pool, move |conn| noti::subscribe_stock(conn, pid, variante_id, &email, uid)).await?;
    Ok(crate::controller::ok_message("Te avisaremos por correo cuando vuelva a estar disponible"))
}

#[derive(Deserialize)]
pub struct LowStockQuery {
    pub umbral: Option<i32>,
}

/// Productos/variantes con stock bajo (para el panel de ventas).
pub async fn admin_low_stock(user: AuthUser, q: web::Query<LowStockQuery>, pool: web::Data<DbPool>) -> ApiResult<HttpResponse> {
    user.require(&[Role::Admin])?;
    let umbral = q.into_inner().umbral.unwrap_or(5);
    Ok(HttpResponse::Ok().json(db::run(&pool, move |conn| noti::low_stock(conn, umbral)).await?))
}

/// Dispara el barrido de notificaciones a mano (además del automático).
pub async fn admin_process(
    user: AuthUser,
    pool: web::Data<DbPool>,
    mailer: web::Data<Mailer>,
    config: web::Data<crate::config::Config>,
) -> ApiResult<HttpResponse> {
    user.require(&[Role::Admin])?;
    let m = mailer.get_ref().clone();
    let fe = config.frontend_url.clone();
    let res = db::run(&pool, move |conn| crate::jobs::run_once(conn, &m, &fe)).await?;
    Ok(HttpResponse::Ok().json(res))
}
