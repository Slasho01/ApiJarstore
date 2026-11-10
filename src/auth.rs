use crate::config::Config;
use crate::errors::ApiError;
use crate::models::enum_role::Role;
use actix_web::{dev::Payload, web, FromRequest, HttpRequest};
use jsonwebtoken::{decode, encode, Algorithm, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use uuid::Uuid;

const ISSUER: &str = "jarstoreapi";

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Claims {
    pub sub: String,
    pub exp: u64,
    pub iat: u64,
    pub iss: String,
    pub rls: Role,
    #[serde(default)]
    pub sv: i32, // versión de sesión (para revocar tokens)
}

pub fn generate_jwt(config: &Config, user_id: Uuid, role: Role, sv: i32) -> Result<String, ApiError> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| ApiError::internal(e.to_string()))?
        .as_secs();
    let claims = Claims {
        sub: user_id.to_string(),
        iat: now,
        exp: now + config.jwt_ttl_seconds,
        iss: ISSUER.to_string(),
        rls: role,
        sv,
    };
    encode(
        &Header::new(Algorithm::HS256),
        &claims,
        &EncodingKey::from_secret(config.jwt_secret.as_bytes()),
    )
    .map_err(|e| ApiError::internal(format!("No se pudo firmar el token: {e}")))
}

#[derive(Serialize, Deserialize)]
struct Pending2faClaims {
    sub: String,     // user id que ya validó la contraseña
    purpose: String, // "2fa"
    exp: u64,
}

/// Token temporal (5 min) emitido tras validar la contraseña, para completar el 2FA en un
/// 2º request SIN reenviar usuario/contraseña.
pub fn create_pending_2fa(config: &Config, user_id: Uuid) -> Result<String, ApiError> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| ApiError::internal(e.to_string()))?
        .as_secs();
    let claims = Pending2faClaims { sub: user_id.to_string(), purpose: "2fa".to_string(), exp: now + 300 };
    encode(&Header::new(Algorithm::HS256), &claims, &EncodingKey::from_secret(config.jwt_secret.as_bytes()))
        .map_err(|e| ApiError::internal(format!("No se pudo firmar el token: {e}")))
}

/// Verifica el token temporal de 2FA y devuelve el id del usuario que validó contraseña.
pub fn verify_pending_2fa(config: &Config, token: &str) -> Result<Uuid, ApiError> {
    let data = decode::<Pending2faClaims>(
        token.trim(),
        &DecodingKey::from_secret(config.jwt_secret.as_bytes()),
        &Validation::new(Algorithm::HS256),
    )
    .map_err(|_| ApiError::unauthorized("La verificación expiró. Vuelve a iniciar sesión."))?;
    if data.claims.purpose != "2fa" {
        return Err(ApiError::unauthorized("Token inválido"));
    }
    Uuid::parse_str(&data.claims.sub).map_err(|_| ApiError::unauthorized("Token inválido"))
}

fn decode_jwt(config: &Config, token: &str) -> Result<Claims, ApiError> {
    let mut validation = Validation::new(Algorithm::HS256);
    validation.set_issuer(&[ISSUER]);
    decode::<Claims>(token, &DecodingKey::from_secret(config.jwt_secret.as_bytes()), &validation)
        .map(|data| data.claims)
        .map_err(|_| ApiError::unauthorized("Token inválido o expirado"))
}

/// Usuario autenticado. Basta con añadir `user: AuthUser` como parámetro de un
/// handler para que la ruta exija `Authorization: Bearer <token>`.
#[derive(Debug, Clone, Copy)]
pub struct AuthUser {
    pub id: Uuid,
    pub role: Role,
}

impl AuthUser {
    pub fn is_admin(&self) -> bool {
        self.role == Role::Admin
    }

    /// Devuelve 403 si el rol del usuario no está en la lista.
    pub fn require(&self, roles: &[Role]) -> Result<(), ApiError> {
        if roles.contains(&self.role) {
            Ok(())
        } else {
            Err(ApiError::forbidden("No tienes permiso para realizar esta acción"))
        }
    }

    /// El recurso pertenece al usuario o el usuario es admin.
    pub fn require_owner_or_admin(&self, owner: Option<Uuid>) -> Result<(), ApiError> {
        if self.is_admin() || owner == Some(self.id) {
            Ok(())
        } else {
            Err(ApiError::forbidden("No eres el dueño de este recurso"))
        }
    }
}

impl FromRequest for AuthUser {
    type Error = ApiError;
    type Future = Pin<Box<dyn Future<Output = Result<Self, Self::Error>>>>;

    fn from_request(req: &HttpRequest, _: &mut Payload) -> Self::Future {
        let req = req.clone();
        Box::pin(async move {
            let (user, token_sv) = extract_user(&req)?;
            // Revalidación por request: el usuario debe existir y su session_version
            // coincidir con el del token (permite revocar sesiones al instante).
            let pool = req
                .app_data::<web::Data<crate::db::DbPool>>()
                .ok_or_else(|| ApiError::internal("Pool no disponible"))?
                .clone();
            let uid = user.id;
            let sv_db: Option<i32> = crate::db::run(&pool, move |conn| {
                use crate::schema::users::dsl as u;
                use diesel::prelude::*;
                u::users.find(uid).select(u::session_version).first::<i32>(conn).optional().map_err(ApiError::from)
            })
            .await?;
            match sv_db {
                Some(sv) if sv == token_sv => Ok(user),
                _ => Err(ApiError::unauthorized("Tu sesión expiró o fue revocada. Inicia sesión de nuevo.")),
            }
        })
    }
}

fn extract_user(req: &HttpRequest) -> Result<(AuthUser, i32), ApiError> {
    let config = req
        .app_data::<web::Data<Config>>()
        .ok_or_else(|| ApiError::internal("Config no registrada en la App"))?;
    let header = req
        .headers()
        .get(actix_web::http::header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
        .ok_or_else(|| ApiError::unauthorized("Falta el header Authorization"))?;
    let token = header
        .strip_prefix("Bearer ")
        .or_else(|| header.strip_prefix("bearer "))
        .ok_or_else(|| ApiError::unauthorized("Formato esperado: Bearer <token>"))?;
    let claims = decode_jwt(config, token.trim())?;
    let id = Uuid::parse_str(&claims.sub).map_err(|_| ApiError::unauthorized("Token inválido"))?;
    Ok((AuthUser { id, role: claims.rls }, claims.sv))
}

/// Limitador sencillo de intentos fallidos de login (en memoria, por clave).
/// Tras `MAX_ATTEMPTS` fallos dentro de `WINDOW`, bloquea hasta que expire la ventana.
pub struct LoginLimiter {
    attempts: Mutex<HashMap<String, (u32, Instant)>>,
}

const MAX_ATTEMPTS: u32 = 5;
const WINDOW: Duration = Duration::from_secs(15 * 60);

impl LoginLimiter {
    pub fn new() -> Self {
        Self { attempts: Mutex::new(HashMap::new()) }
    }

    pub fn check(&self, key: &str) -> Result<(), ApiError> {
        let mut map = self.attempts.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((count, since)) = map.get(key) {
            if since.elapsed() > WINDOW {
                map.remove(key);
            } else if *count >= MAX_ATTEMPTS {
                return Err(ApiError::TooManyRequests(
                    "Demasiados intentos fallidos. Intenta de nuevo en unos minutos".into(),
                ));
            }
        }
        Ok(())
    }

    pub fn register_failure(&self, key: &str) {
        let mut map = self.attempts.lock().unwrap_or_else(|e| e.into_inner());
        // Limpieza oportunista para que el mapa no crezca sin límite
        if map.len() > 10_000 {
            map.retain(|_, (_, since)| since.elapsed() <= WINDOW);
        }
        let entry = map.entry(key.to_string()).or_insert((0, Instant::now()));
        if entry.1.elapsed() > WINDOW {
            *entry = (0, Instant::now());
        }
        entry.0 += 1;
    }

    pub fn reset(&self, key: &str) {
        self.attempts.lock().unwrap_or_else(|e| e.into_inner()).remove(key);
    }
}
