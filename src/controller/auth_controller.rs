use crate::auth::{generate_jwt, LoginLimiter};
use crate::config::Config;
use crate::db::{self, DbPool};
use crate::errors::{ApiError, ApiResult};
use crate::mailer::Mailer;
use crate::models::users::UserResponse;
use crate::services::user_services::{self, RegisterRequest};
use actix_web::{web, HttpRequest, HttpResponse};
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
pub struct LoginRequest {
    /// username o email
    #[serde(alias = "email")]
    pub username: String,
    pub password: String,
    #[serde(default)]
    pub codigo: Option<String>,
}

#[derive(Serialize)]
struct LoginResponse {
    token: String,
    token_type: &'static str,
    expires_in: u64,
    user: UserResponse,
}

// Hash bcrypt válido de una contraseña aleatoria: se verifica cuando el usuario
// no existe para que el tiempo de respuesta no revele si existe o no.
const DUMMY_HASH: &str = "$2b$12$LQv3c1yqBWVHxkd0LHAkCOYz6TtxMQJqhN8/LewE9i.yquG5/w/Y6";

pub async fn login(
    body: web::Json<LoginRequest>,
    pool: web::Data<DbPool>,
    config: web::Data<Config>,
    limiter: web::Data<LoginLimiter>,
    req: HttpRequest,
) -> ApiResult<HttpResponse> {
    let LoginRequest { username, password, codigo } = body.into_inner();
    let ip = req.connection_info().realip_remote_addr().unwrap_or("?").to_string();
    let key = format!("{}|{}", username.trim().to_lowercase(), ip);
    limiter.check(&key)?;

    let user = db::run(&pool, move |conn| {
        let user = user_services::find_by_login(conn, &username)?;
        let ok = match &user {
            Some(u) => bcrypt::verify(&password, &u.password_hash).unwrap_or(false),
            None => {
                let _ = bcrypt::verify(&password, DUMMY_HASH);
                false
            }
        };
        Ok(if ok { user } else { None })
    })
    .await?;

    match user {
        Some(user) => {
            // 2FA: si está activo, exige el código TOTP.
            if user.twofa_enabled {
                match codigo.as_deref() {
                    None => {
                        limiter.reset(&key);
                        return Ok(HttpResponse::Ok().json(serde_json::json!({ "twofa_required": true })));
                    }
                    Some(c) => {
                        if !crate::twofa::verificar(&crate::crypto::descifrar(user.twofa_secret.as_deref().unwrap_or("")), c) {
                            limiter.register_failure(&key);
                            return Err(ApiError::unauthorized("Código de verificación inválido"));
                        }
                    }
                }
            }
            limiter.reset(&key);
            log::info!("login ok: {}", user.username);
            let token = generate_jwt(&config, user.id, user.role, user.session_version)?;
            Ok(HttpResponse::Ok().json(LoginResponse {
                token,
                token_type: "Bearer",
                expires_in: config.jwt_ttl_seconds,
                user: user.into(),
            }))
        }
        None => {
            limiter.register_failure(&key);
            log::warn!("login fallido desde {ip}");
            Err(ApiError::unauthorized("Credenciales inválidas"))
        }
    }
}

pub async fn register(body: web::Json<RegisterRequest>, pool: web::Data<DbPool>) -> ApiResult<HttpResponse> {
    let user = db::run(&pool, move |conn| user_services::register(conn, body.into_inner())).await?;
    Ok(HttpResponse::Created().json(user))
}

#[derive(Deserialize)]
pub struct ForgotRequest {
    pub email: String,
}

/// Siempre responde 200 (no revela si el email existe).
pub async fn forgot_password(
    body: web::Json<ForgotRequest>,
    pool: web::Data<DbPool>,
    config: web::Data<Config>,
    mailer: web::Data<Mailer>,
    limiter: web::Data<LoginLimiter>,
    req: HttpRequest,
) -> ApiResult<HttpResponse> {
    let ip = req.connection_info().realip_remote_addr().unwrap_or("?").to_string();
    let key = format!("forgot|{ip}");
    limiter.check(&key)?;
    limiter.register_failure(&key); // máx. 5 solicitudes por IP cada 15 min

    let email = body.into_inner().email;
    let created = db::run(&pool, move |conn| user_services::create_reset_token(conn, &email)).await?;
    if let Some((user, token)) = created {
        let link = format!("{}/restablecer?token={token}", config.frontend_url);
        let tienda = "JarStore";
        let text = format!(
            "Hola {},\n\nRecibimos una solicitud para restablecer tu contraseña en {tienda}.\n\
             Abre este enlace (válido por {} minutos):\n{link}\n\nSi no fuiste tú, ignora este correo.",
            user.username, user_services::RESET_TTL);
        let html = format!(
            "<p>Hola {},</p><p>Recibimos una solicitud para restablecer tu contraseña en {tienda}.</p>\
             <p><a href=\"{link}\">Restablecer contraseña</a> (válido por {} minutos)</p>\
             <p>Si no fuiste tú, ignora este correo.</p>",
            html_escape(&user.username), user_services::RESET_TTL);
        let mailer = mailer.clone();
        let to = user.email.clone();
        let res = web::block(move || mailer.send(&to, "Restablece tu contraseña", &text, &html)).await;
        if let Err(e) | Ok(Err(e)) = res.map_err(|e| e.to_string()) {
            log::error!("No se pudo enviar el correo de recuperación: {e}");
        }
    }
    Ok(HttpResponse::Ok().json(serde_json::json!({
        "message": "Si el email está registrado, te enviamos un enlace para restablecer la contraseña"
    })))
}

#[derive(Deserialize)]
pub struct ResetRequest {
    pub token: String,
    pub password: String,
}

pub async fn reset_password(body: web::Json<ResetRequest>, pool: web::Data<DbPool>) -> ApiResult<HttpResponse> {
    let ResetRequest { token, password } = body.into_inner();
    db::run(&pool, move |conn| user_services::reset_password(conn, &token, &password)).await?;
    Ok(HttpResponse::Ok().json(serde_json::json!({ "message": "Contraseña actualizada. Ya puedes iniciar sesión" })))
}

pub fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}
