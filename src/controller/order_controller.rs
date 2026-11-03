use crate::auth::AuthUser;
use crate::db::{self, DbPool};
use crate::errors::ApiResult;
use crate::models::enum_role::Role;
use crate::models::orden::EnvioData;
use crate::services::common::Pagination;
use crate::controller::auth_controller::html_escape;
use crate::mailer::Mailer;
use crate::storage::{Storage, MAX_DOC_BYTES};
use crate::services::upload_services;
use actix_multipart::Multipart;
use futures_util::StreamExt;
use crate::payments::MercadoPago;
use crate::errors::ApiError;
use actix_web::HttpRequest;
use serde::Deserialize;
use crate::services::ordenes_services::{self as svc, CheckoutRequest, OrdenDetalle, OrderFilter, PagoRequest, QuoteRequest, StatusRequest};
use actix_web::{web, HttpResponse};
use uuid::Uuid;

pub async fn quote(user: AuthUser, body: web::Json<QuoteRequest>, pool: web::Data<DbPool>, courier: web::Data<crate::courier::Courier>) -> ApiResult<HttpResponse> {
    let c = courier.get_ref().clone();
    Ok(HttpResponse::Ok().json(db::run(&pool, move |conn| svc::quote(conn, user.id, body.into_inner(), &c)).await?))
}

pub async fn checkout(
    user: AuthUser,
    body: web::Json<CheckoutRequest>,
    pool: web::Data<DbPool>,
    mailer: web::Data<Mailer>,
    config: web::Data<crate::config::Config>,
    pay: web::Data<Payments>,
    courier: web::Data<crate::courier::Courier>,
) -> ApiResult<HttpResponse> {
    let mp = pay.mp.is_some();
    let c = courier.get_ref().clone();
    let orden = db::run(&pool, move |conn| svc::checkout(conn, user.id, body.into_inner(), mp, &c)).await?;
    match db::run(&pool, crate::services::tienda_services::monedas).await {
        Ok(monedas) => send_confirmation(&mailer, &config.frontend_url, &orden, &monedas),
        Err(e) => log::error!("No se pudo preparar el correo de confirmación: {e}"),
    }
    notify_store_new_order(&mailer, &pool, &config.frontend_url, &orden);
    Ok(HttpResponse::Created().json(orden))
}

/// Correo de confirmación (en segundo plano; si falla sólo se registra en el log).
fn send_confirmation(mailer: &Mailer, frontend_url: &str, o: &OrdenDetalle, monedas: &crate::models::tienda::MonedasConfig) {
    use crate::services::tienda_services::format_money;
    let base = monedas.lista.iter().find(|m| m.codigo == monedas.base);
    let base_dec = base.map_or(0, |m| m.decimales);
    let fmt = |amount: i64| match monedas.lista.iter().find(|m| m.codigo == o.orden.moneda).or(base) {
        Some(m) => format_money(amount, m, o.orden.tasa_cambio, base_dec),
        None => amount.to_string(),
    };
    let Some(to) = o.orden.email_comprador.clone() else { return };
    let short = o.orden.id.to_string()[..8].to_string();
    let link = format!("{frontend_url}/cuenta/pedidos/{}", o.orden.id);
    let lines: String = o.detalles.iter()
        .map(|d| format!("- {} x{}{}", d.nombre_producto, d.cantidad, d.nombre_variante.as_ref().map(|v| format!(" ({v})")).unwrap_or_default()))
        .collect::<Vec<_>>().join("\n");
    let text = format!(
        "¡Gracias por tu compra!\n\nPedido #{short}\n{lines}\n\nTotal: {}\nPago: {}\nEnvío: {}\n\nRevisa el estado aquí: {link}",
        fmt(o.orden.total), o.orden.metodo_pago.as_deref().unwrap_or("-"),
        o.orden.metodo_envio.as_deref().unwrap_or("-"));
    let html = format!(
        "<h2>¡Gracias por tu compra!</h2><p>Pedido <b>#{short}</b></p><pre>{}</pre><p><a href=\"{link}\">Ver estado del pedido</a></p>",
        html_escape(&text));
    let mailer = mailer.clone();
    actix_web::rt::task::spawn_blocking(move || {
        if let Err(e) = mailer.send(&to, &format!("Confirmación de tu pedido #{short}"), &text, &html) {
            log::error!("No se pudo enviar la confirmación del pedido #{short}: {e}");
        }
    });
}

/// Correo al cliente cuando cambia el estado del pedido (en segundo plano).
fn send_status_email(mailer: &web::Data<Mailer>, frontend_url: &str, o: &OrdenDetalle, status: &str) {
    let Some(to) = o.orden.email_comprador.clone() else { return };
    let short = o.orden.id.to_string()[..8].to_uppercase();
    let link = format!("{frontend_url}/cuenta/pedidos/{}", o.orden.id);
    let primer = o.orden.nombre_comprador.as_deref().and_then(|n| n.split_whitespace().next()).unwrap_or("");
    let saludo = if primer.is_empty() { "Hola".to_string() } else { format!("Hola {primer}") };
    let (subject, titulo, cuerpo) = match status {
        "pagado" => (format!("Pago confirmado — Pedido #{short}"), "¡Pago confirmado!", "Recibimos el pago de tu pedido y ya lo estamos preparando.".to_string()),
        "enviado" => {
            let track = o.envio.as_ref().map(|e| {
                let mut p = vec![];
                if let Some(x) = &e.empresa_envio { p.push(format!("Empresa: {x}")); }
                if let Some(x) = &e.codigo_seguimiento { p.push(format!("Seguimiento: {x}")); }
                p.join(" · ")
            }).filter(|s| !s.is_empty()).map(|s| format!("\n{s}")).unwrap_or_default();
            (format!("Tu pedido va en camino — #{short}"), "¡Tu pedido va en camino!", format!("Tu pedido fue despachado.{track}"))
        }
        "entregado" => (format!("Pedido entregado — #{short}"), "¡Pedido entregado!", "Tu pedido fue marcado como entregado. ¡Gracias por tu compra!".to_string()),
        "cancelado" => (format!("Pedido cancelado — #{short}"), "Pedido cancelado", "Tu pedido fue cancelado. Si habías realizado un pago, la tienda gestionará la devolución.".to_string()),
        _ => return,
    };
    let text = format!("{saludo},\n\n{cuerpo}\n\nPedido #{short}\nVer el detalle: {link}");
    let html = format!("<h2>{}</h2><p>{saludo},</p><p>{}</p><p>Pedido <b>#{short}</b></p><p><a href=\"{link}\">Ver el detalle del pedido</a></p>",
        html_escape(titulo), html_escape(&cuerpo).replace('\n', "<br>"));
    let mailer = mailer.clone();
    let status = status.to_string();
    actix_web::rt::task::spawn_blocking(move || {
        if let Err(e) = mailer.send(&to, &subject, &text, &html) {
            log::error!("No se pudo enviar el correo de estado ({status}) del pedido #{short}: {e}");
        }
    });
}

/// Aviso a la tienda cuando entra un pedido nuevo (si hay email de tienda configurado).
fn notify_store_new_order(mailer: &web::Data<Mailer>, pool: &web::Data<DbPool>, frontend_url: &str, o: &OrdenDetalle) {
    let mailer = mailer.clone();
    let pool = pool.clone();
    let short = o.orden.id.to_string()[..8].to_uppercase();
    let id = o.orden.id;
    let comprador = o.orden.nombre_comprador.clone().unwrap_or_default();
    let fe = frontend_url.to_string();
    actix_web::rt::task::spawn_blocking(move || {
        let Ok(mut conn) = pool.get() else { return };
        let Ok(tienda) = crate::services::tienda_services::tienda(&mut conn) else { return };
        let to = tienda.email.trim().to_string();
        if to.is_empty() { return; }
        let link = format!("{fe}/admin/ordenes/{id}");
        let text = format!("Nuevo pedido #{short} de {comprador}.\nGestiónalo aquí: {link}");
        let html = format!("<h2>Nuevo pedido #{short}</h2><p>Cliente: {}</p><p><a href=\"{link}\">Gestionar pedido</a></p>", html_escape(&comprador));
        if let Err(e) = mailer.send(&to, &format!("Nuevo pedido #{short}"), &text, &html) {
            log::error!("No se pudo avisar a la tienda del pedido #{short}: {e}");
        }
    });
}

pub async fn mine(user: AuthUser, page: web::Query<Pagination>, pool: web::Data<DbPool>) -> ApiResult<HttpResponse> {
    let p = page.into_inner();
    let f = OrderFilter { estatus: None, user_id: Some(user.id) };
    Ok(HttpResponse::Ok().json(db::run(&pool, move |conn| svc::list(conn, f, p)).await?))
}

pub async fn get(path: web::Path<Uuid>, user: AuthUser, pool: web::Data<DbPool>) -> ApiResult<HttpResponse> {
    let id = path.into_inner();
    Ok(HttpResponse::Ok().json(db::run(&pool, move |conn| svc::get_for(conn, id, &user)).await?))
}

pub async fn cancel(path: web::Path<Uuid>, user: AuthUser, pool: web::Data<DbPool>, mailer: web::Data<Mailer>, config: web::Data<crate::config::Config>) -> ApiResult<HttpResponse> {
    let id = path.into_inner();
    let orden = db::run(&pool, move |conn| svc::cancel(conn, id, user)).await?;
    send_status_email(&mailer, &config.frontend_url, &orden, "cancelado");
    Ok(HttpResponse::Ok().json(orden))
}

// ---------- Admin ----------

pub async fn admin_list(
    user: AuthUser,
    filter: web::Query<OrderFilter>,
    page: web::Query<Pagination>,
    pool: web::Data<DbPool>,
) -> ApiResult<HttpResponse> {
    user.require(&[Role::Admin])?;
    let (f, p) = (filter.into_inner(), page.into_inner());
    Ok(HttpResponse::Ok().json(db::run(&pool, move |conn| svc::list(conn, f, p)).await?))
}

pub async fn admin_set_status(
    path: web::Path<Uuid>,
    user: AuthUser,
    body: web::Json<StatusRequest>,
    pool: web::Data<DbPool>,
    mailer: web::Data<Mailer>,
    config: web::Data<crate::config::Config>,
) -> ApiResult<HttpResponse> {
    user.require(&[Role::Admin])?;
    let (id, next) = (path.into_inner(), body.estatus);
    let orden = db::run(&pool, move |conn| svc::set_status(conn, id, next)).await?;
    send_status_email(&mailer, &config.frontend_url, &orden, next.as_str());
    Ok(HttpResponse::Ok().json(orden))
}

pub async fn admin_add_payment(
    path: web::Path<Uuid>,
    user: AuthUser,
    body: web::Json<PagoRequest>,
    pool: web::Data<DbPool>,
    mailer: web::Data<Mailer>,
    config: web::Data<crate::config::Config>,
) -> ApiResult<HttpResponse> {
    user.require(&[Role::Admin])?;
    let id = path.into_inner();
    let (orden, just_paid) = db::run(&pool, move |conn| svc::add_payment(conn, id, body.into_inner())).await?;
    if just_paid {
        send_status_email(&mailer, &config.frontend_url, &orden, "pagado");
    }
    Ok(HttpResponse::Created().json(orden))
}

pub async fn admin_set_shipping(
    path: web::Path<Uuid>,
    user: AuthUser,
    body: web::Json<EnvioData>,
    pool: web::Data<DbPool>,
) -> ApiResult<HttpResponse> {
    user.require(&[Role::Admin])?;
    let id = path.into_inner();
    Ok(HttpResponse::Ok().json(db::run(&pool, move |conn| svc::set_shipping(conn, id, body.into_inner())).await?))
}


// ---------- Pagos online (Mercado Pago) ----------

/// Integraciones de pago disponibles en el servidor.
pub struct Payments {
    pub mp: Option<MercadoPago>,
}

fn mp(pay: &Payments) -> Result<&MercadoPago, ApiError> {
    pay.mp.as_ref().ok_or_else(|| ApiError::bad_request("Mercado Pago no está configurado en el servidor"))
}

/// POST /api/ordenes/{id}/pagar -> URL de Checkout Pro para pagar la orden.
pub async fn pay(path: web::Path<Uuid>, user: AuthUser, pool: web::Data<DbPool>, pay: web::Data<Payments>) -> ApiResult<HttpResponse> {
    let mp = mp(&pay)?;
    let id = path.into_inner();
    let (orden, tienda, base) = db::run(&pool, move |conn| {
        let o = svc::payable_order(conn, id, &user)?;
        Ok((o, crate::services::tienda_services::tienda(conn)?.nombre, crate::services::tienda_services::monedas(conn)?.base))
    }).await?;
    let pref = mp.create_preference(&orden, &tienda, &base).await?;
    Ok(HttpResponse::Ok().json(serde_json::json!({ "init_point": pref.init_point, "preference_id": pref.id })))
}

#[derive(Deserialize)]
pub struct VerifyBody {
    pub payment_id: String,
}

/// Sincroniza un pago al volver de Mercado Pago (útil si el webhook aún no llegó).
pub async fn verify_payment(path: web::Path<Uuid>, user: AuthUser, body: web::Json<VerifyBody>, pool: web::Data<DbPool>, pay: web::Data<Payments>, mailer: web::Data<Mailer>, config: web::Data<crate::config::Config>) -> ApiResult<HttpResponse> {
    let mp = mp(&pay)?;
    let id = path.into_inner();
    db::run(&pool, move |conn| svc::get_for(conn, id, &user)).await?; // permisos
    let p = mp.get_payment(body.payment_id.trim()).await?;
    if p.external_reference.as_deref() != Some(&id.to_string()) {
        return Err(ApiError::bad_request("El pago no corresponde a este pedido"));
    }
    let (orden, just_paid) = apply_mp(&pool, p).await?;
    if just_paid {
        send_status_email(&mailer, &config.frontend_url, &orden, "pagado");
    }
    Ok(HttpResponse::Ok().json(orden))
}

async fn apply_mp(pool: &web::Data<DbPool>, p: crate::payments::MpPago) -> ApiResult<(svc::OrdenDetalle, bool)> {
    let orden_id = p.external_reference.as_deref().and_then(|r| Uuid::parse_str(r).ok())
        .ok_or_else(|| ApiError::bad_request("Pago sin referencia a un pedido"))?;
    let monto = p.transaction_amount.round() as i64;
    db::run(pool, move |conn| svc::apply_provider_payment(
        conn, "mercadopago", &p.id.to_string(), orden_id, monto, &p.currency_id, &p.status,
        Some(format!("Mercado Pago{}", p.payment_method_id.as_deref().map(|m| format!(" ({m})")).unwrap_or_default())),
    )).await
}

#[derive(Deserialize)]
pub struct WebhookQuery {
    #[serde(rename = "data.id")]
    pub data_id: Option<String>,
    #[serde(rename = "type")]
    pub kind: Option<String>,
    pub topic: Option<String>,
    pub id: Option<String>,
}

/// POST /api/pagos/mercadopago/webhook — notificaciones de Mercado Pago.
pub async fn mp_webhook(req: HttpRequest, q: web::Query<WebhookQuery>, body: web::Bytes, pool: web::Data<DbPool>, pay: web::Data<Payments>, mailer: web::Data<Mailer>, config: web::Data<crate::config::Config>) -> ApiResult<HttpResponse> {
    let Some(mp) = pay.mp.as_ref() else { return Ok(HttpResponse::Ok().finish()) };
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap_or_default();
    let kind = q.kind.clone().or(q.topic.clone()).or(json["type"].as_str().map(String::from)).unwrap_or_default();
    let data_id = q.data_id.clone()
        .or(json["data"]["id"].as_str().map(String::from))
        .or(json["data"]["id"].as_i64().map(|i| i.to_string()))
        .or(q.id.clone())
        .unwrap_or_default();
    if kind != "payment" || data_id.is_empty() {
        return Ok(HttpResponse::Ok().finish()); // otros eventos se ignoran
    }
    let header = |n: &str| req.headers().get(n).and_then(|v| v.to_str().ok());
    if !mp.verify_webhook(header("x-signature"), header("x-request-id"), &data_id) {
        log::warn!("Webhook de Mercado Pago con firma inválida (data.id={data_id})");
        return Err(ApiError::unauthorized("Firma inválida"));
    }
    match mp.get_payment(&data_id).await {
        Ok(p) => match apply_mp(&pool, p).await {
            Ok((orden, just_paid)) => {
                if just_paid {
                    send_status_email(&mailer, &config.frontend_url, &orden, "pagado");
                }
            }
            Err(e) => {
                log::error!("No se pudo aplicar el pago {data_id}: {e}");
                return Err(e); // 4xx/5xx => Mercado Pago reintenta
            }
        },
        Err(e) => {
            log::error!("No se pudo consultar el pago {data_id}: {e}");
            return Err(e);
        }
    }
    Ok(HttpResponse::Ok().finish())
}


/// POST /api/admin/ordenes/{id}/documento (multipart `file`): sube la boleta/factura (PDF o imagen).
pub async fn admin_set_document(
    path: web::Path<Uuid>,
    user: AuthUser,
    mut payload: Multipart,
    pool: web::Data<DbPool>,
    storage: web::Data<Storage>,
) -> ApiResult<HttpResponse> {
    user.require(&[crate::models::enum_role::Role::Admin])?;
    let id = path.into_inner();
    let mut file: Option<Vec<u8>> = None;
    while let Some(field) = payload.next().await {
        let mut field = field.map_err(|e| ApiError::bad_request(format!("Formulario inválido: {e}")))?;
        if field.name() != Some("file") { continue; }
        let mut buf = Vec::new();
        while let Some(chunk) = field.next().await {
            let chunk = chunk.map_err(|e| ApiError::bad_request(format!("Error leyendo el archivo: {e}")))?;
            if buf.len() + chunk.len() > MAX_DOC_BYTES {
                return Err(ApiError::bad_request("El documento supera los 15 MB"));
            }
            buf.extend_from_slice(&chunk);
        }
        file = Some(buf);
    }
    let data = file.ok_or_else(|| ApiError::bad_request("Falta el archivo (campo 'file')"))?;
    let stored = storage.store_document(data, &format!("documento-{}", &id.to_string()[..8])).await?;
    let url = stored.url.clone();
    let uid = user.id;
    let orden = db::run(&pool, move |conn| {
        upload_services::record(conn, uid, &stored).ok();
        svc::set_document(conn, id, url)
    }).await?;
    Ok(HttpResponse::Ok().json(orden))
}
