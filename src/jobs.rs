//! Tareas periódicas de conversión: avisos de reposición de stock y
//! recordatorios de carrito abandonado. El barrido corre cada
//! `JOBS_INTERVAL_SECS` (por defecto 900 s) y también se puede disparar a mano
//! desde el panel (POST /api/admin/notificaciones/procesar).
use crate::controller::auth_controller::html_escape;
use crate::db::DbPool;
use crate::errors::{ApiError, ApiResult};
use crate::mailer::Mailer;
use crate::services::notificaciones_services as noti;
use diesel::PgConnection;
use std::time::Duration;

/// Antigüedad mínima (horas) del carrito para enviar el recordatorio.
const CART_MIN_HORAS: i64 = 2;
/// Antigüedad máxima (días): más viejo que esto ya no se molesta al cliente.
const CART_MAX_DIAS: i64 = 7;

#[derive(serde::Serialize, Debug, Default)]
pub struct JobResult {
    pub avisos_stock: usize,
    pub recordatorios_carrito: usize,
}

/// Envía los avisos de stock y los recordatorios de carrito que correspondan.
pub fn run_once(conn: &mut PgConnection, mailer: &Mailer, frontend_url: &str) -> ApiResult<JobResult> {
    let fe = frontend_url.trim_end_matches('/');

    let alerts = noti::due_stock_alerts(conn)?;
    for a in &alerts {
        let link = format!("{fe}/producto/{}", a.slug);
        let variante = a.variante.as_deref().map(|v| format!(" ({v})")).unwrap_or_default();
        let subject = format!("¡{} volvió a estar disponible!", a.producto_nombre);
        let text = format!("{}{} ya está disponible de nuevo en JarStore. Cómpralo aquí: {link}", a.producto_nombre, variante);
        let html = format!(
            "<p><b>{}</b>{} ya volvió a estar disponible en JarStore.</p><p><a href=\"{link}\">Ver producto y comprar</a></p><p style=\"color:#888;font-size:12px\">Recibes este correo porque pediste que te avisáramos.</p>",
            html_escape(&a.producto_nombre), html_escape(&variante)
        );
        if let Err(e) = mailer.send(&a.email, &subject, &text, &html) {
            log::warn!("No se pudo enviar aviso de stock a {}: {e}", a.email);
        }
    }

    let reminders = noti::due_cart_reminders(conn, CART_MIN_HORAS, CART_MAX_DIAS)?;
    for r in &reminders {
        let lista = r.items.iter().map(|(n, c)| format!("{c} × {n}")).collect::<Vec<_>>().join(", ");
        let items_html = r.items.iter().map(|(n, c)| format!("<li>{c} × {}</li>", html_escape(n))).collect::<String>();
        let link = format!("{fe}/carrito");
        let hola = r.nombre.as_deref().map(|n| format!("Hola {n}, ")).unwrap_or_default();
        let subject = "¿Olvidaste algo en tu carrito?";
        let text = format!("{hola}dejaste productos en tu carrito: {lista}. Termina tu compra aquí: {link}");
        let html = format!(
            "<p>{}dejaste estos productos en tu carrito de JarStore:</p><ul>{items_html}</ul><p><a href=\"{link}\">Finalizar mi compra</a></p>",
            html_escape(&hola)
        );
        if let Err(e) = mailer.send(&r.email, subject, &text, &html) {
            log::warn!("No se pudo enviar recordatorio de carrito a {}: {e}", r.email);
        }
    }

    Ok(JobResult { avisos_stock: alerts.len(), recordatorios_carrito: reminders.len() })
}

/// Lanza el barrido periódico en segundo plano. No bloquea el arranque.
pub fn start(pool: DbPool, mailer: Mailer, frontend_url: String) {
    let secs = std::env::var("JOBS_INTERVAL_SECS").ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(900)
        .max(30);
    log::info!("Tareas de conversión (avisos de stock, carrito abandonado) cada {secs}s");
    actix_web::rt::spawn(async move {
        let mut tick = actix_web::rt::time::interval(Duration::from_secs(secs));
        tick.tick().await; // consumir el tick inmediato: el primer barrido corre tras `secs`
        loop {
            tick.tick().await;
            let (pool, mailer, fe) = (pool.clone(), mailer.clone(), frontend_url.clone());
            let res = actix_web::web::block(move || {
                let mut conn = pool.get().map_err(|e| ApiError::internal(e.to_string()))?;
                run_once(&mut conn, &mailer, &fe)
            }).await;
            match res {
                Ok(Ok(j)) if j.avisos_stock + j.recordatorios_carrito > 0 =>
                    log::info!("Notificaciones enviadas: {} avisos de stock, {} recordatorios de carrito", j.avisos_stock, j.recordatorios_carrito),
                Ok(Ok(_)) => {}
                Ok(Err(e)) => log::warn!("Error en tareas de conversión: {e}"),
                Err(e) => log::warn!("Error ejecutando tareas de conversión: {e}"),
            }
        }
    });
}
