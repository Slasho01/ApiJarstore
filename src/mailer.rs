//! Envío de correos. La configuración SMTP se toma de la base de datos (editable en
//! Admin → Tienda → Correo); si no hay una activa, se usan las variables de entorno
//! (SMTP_HOST, …) como respaldo; y si tampoco hay, los correos se escriben en el log.
use crate::db::DbPool;
use crate::models::tienda::SmtpConfig;
use crate::schema::configuracion;
use diesel::prelude::*;
use lettre::message::{header::ContentType, Mailbox, MultiPart, SinglePart};
use lettre::transport::smtp::authentication::Credentials;
use lettre::{Message, SmtpTransport, Transport};
use std::env;

#[derive(Clone)]
pub struct Mailer {
    pool: DbPool,
    /// Config tomada del entorno al arrancar (respaldo si no hay config activa en la BD)
    env_config: Option<SmtpConfig>,
}

impl Mailer {
    pub fn new(pool: DbPool) -> Self {
        Mailer { pool, env_config: Self::from_env() }
    }

    fn from_env() -> Option<SmtpConfig> {
        let host = env::var("SMTP_HOST").ok().filter(|h| !h.trim().is_empty())?;
        let (from_nombre, from_email) = parse_from(&env::var("SMTP_FROM").unwrap_or_default());
        log::info!("SMTP de respaldo cargado del entorno ({host})");
        Some(SmtpConfig {
            activo: true,
            host,
            port: env::var("SMTP_PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(587),
            security: env::var("SMTP_SECURITY").unwrap_or_else(|_| "starttls".into()).to_lowercase(),
            usuario: env::var("SMTP_USER").unwrap_or_default(),
            password: env::var("SMTP_PASSWORD").unwrap_or_default(),
            from_nombre: if from_nombre.is_empty() { "JarStore".into() } else { from_nombre },
            from_email: if from_email.is_empty() { format!("no-reply@{}", env::var("SMTP_HOST").unwrap_or_default()) } else { from_email },
        })
    }

    /// Config efectiva: la de la BD si está activa, si no la del entorno.
    pub fn effective_config(&self, conn: &mut PgConnection) -> Option<SmtpConfig> {
        let db: Option<SmtpConfig> = configuracion::table
            .find("smtp")
            .select(configuracion::valor)
            .first::<serde_json::Value>(conn)
            .ok()
            .and_then(|v| serde_json::from_value(v).ok());
        match db {
            Some(c) if c.usable() => Some(c),
            _ => self.env_config.clone(),
        }
    }

    /// ¿Hay correo configurado? (usa una conexión; llamar fuera de un hilo async pesado)
    pub fn is_configured(&self, conn: &mut PgConnection) -> bool {
        self.effective_config(conn).is_some()
    }

    /// Envía un correo (bloqueante: llamar desde web::block / spawn_blocking).
    /// Devuelve Ok aun en modo log (desarrollo). `send_strict` sí reporta el error real.
    pub fn send(&self, to: &str, subject: &str, text: &str, html: &str) -> Result<(), String> {
        match self.send_strict(to, subject, text, html) {
            Ok(sent) => {
                if !sent {
                    log::info!("📧 [correo de desarrollo] Para: {to}\nAsunto: {subject}\n{text}");
                }
                Ok(())
            }
            Err(e) => Err(e),
        }
    }

    /// Envía y reporta el error real. `Ok(false)` = no hay SMTP configurado (no se envió).
    pub fn send_strict(&self, to: &str, subject: &str, text: &str, html: &str) -> Result<bool, String> {
        let mut conn = self.pool.get().map_err(|e| format!("Sin conexión a la BD: {e}"))?;
        let Some(cfg) = self.effective_config(&mut conn) else {
            return Ok(false);
        };
        let (transport, from) = build_transport(&cfg)?;
        let to: Mailbox = to.trim().parse().map_err(|e| format!("Destinatario inválido: {e}"))?;
        let email = Message::builder()
            .from(from)
            .to(to)
            .subject(subject)
            .multipart(
                MultiPart::alternative()
                    .singlepart(SinglePart::builder().header(ContentType::TEXT_PLAIN).body(text.to_string()))
                    .singlepart(SinglePart::builder().header(ContentType::TEXT_HTML).body(html.to_string())),
            )
            .map_err(|e| e.to_string())?;
        transport.send(&email).map(|_| true).map_err(|e| e.to_string())
    }
}

fn parse_from(raw: &str) -> (String, String) {
    // "Nombre <correo@dom>" o "correo@dom"
    match raw.rsplit_once('<') {
        Some((name, rest)) => (name.trim().trim_matches('"').to_string(), rest.trim_end_matches('>').trim().to_string()),
        None => (String::new(), raw.trim().to_string()),
    }
}

/// Construye el transporte SMTP y el remitente a partir de la config.
pub fn build_transport(cfg: &SmtpConfig) -> Result<(SmtpTransport, Mailbox), String> {
    let from_str = if cfg.from_nombre.trim().is_empty() {
        cfg.from_email.clone()
    } else {
        format!("{} <{}>", cfg.from_nombre.trim(), cfg.from_email.trim())
    };
    let from: Mailbox = from_str.parse().map_err(|e| format!("Remitente inválido ('{from_str}'): {e}"))?;
    let mut builder = match cfg.security.as_str() {
        "tls" => SmtpTransport::relay(&cfg.host),
        "none" => Ok(SmtpTransport::builder_dangerous(&cfg.host)),
        _ => SmtpTransport::starttls_relay(&cfg.host),
    }
    .map_err(|e| format!("Servidor SMTP inválido: {e}"))?
    .port(cfg.port);
    if !cfg.usuario.trim().is_empty() {
        builder = builder.credentials(Credentials::new(cfg.usuario.clone(), cfg.password.clone()));
    }
    Ok((builder.build(), from))
}
