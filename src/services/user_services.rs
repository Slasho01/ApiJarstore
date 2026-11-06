use crate::errors::{ApiError, ApiResult, OrNotFound};
use crate::models::enum_role::Role;
use crate::models::users::{NewUser, User, UserResponse};
use crate::schema::users;
use crate::services::common::{now, required_text, Paginated, Pagination};
use diesel::prelude::*;
use diesel::result::{DatabaseErrorKind, Error as DieselError};
use serde::Deserialize;
use uuid::Uuid;

#[derive(Deserialize)]
pub struct RegisterRequest {
    pub username: String,
    pub email: String,
    pub password: String,
}

#[derive(Deserialize)]
pub struct ChangePasswordRequest {
    pub old_password: String,
    pub new_password: String,
    pub confirm_new_password: String,
}

#[derive(Deserialize)]
pub struct UserFilter {
    pub q: Option<String>,
    pub role: Option<Role>,
}

pub fn validate_password(password: &str) -> ApiResult<()> {
    if password.chars().count() < 8 {
        return Err(ApiError::bad_request("La contraseña debe tener al menos 8 caracteres"));
    }
    // bcrypt sólo usa los primeros 72 bytes
    if password.len() > 72 {
        return Err(ApiError::bad_request("La contraseña admite máximo 72 bytes"));
    }
    Ok(())
}

fn validate_email(email: &str) -> ApiResult<String> {
    let email = required_text("email", email, 255)?.to_lowercase();
    let valid = match email.split_once('@') {
        Some((local, domain)) => !local.is_empty() && domain.contains('.') && !domain.starts_with('.') && !domain.ends_with('.'),
        None => false,
    };
    if !valid || email.contains(char::is_whitespace) {
        return Err(ApiError::bad_request("Email inválido"));
    }
    Ok(email)
}

fn validate_username(username: &str) -> ApiResult<String> {
    let u = required_text("username", username, 50)?;
    if u.chars().count() < 3 || !u.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '.' || c == '-') {
        return Err(ApiError::bad_request(
            "El username debe tener 3-50 caracteres: letras, números, '_', '.' o '-'",
        ));
    }
    Ok(u)
}

pub fn hash_password(password: &str) -> ApiResult<String> {
    bcrypt::hash(password, bcrypt::DEFAULT_COST).map_err(|e| ApiError::internal(format!("bcrypt: {e}")))
}

pub fn register(conn: &mut PgConnection, req: RegisterRequest) -> ApiResult<UserResponse> {
    let new_user = NewUser {
        username: validate_username(&req.username)?,
        email: validate_email(&req.email)?,
        password_hash: {
            validate_password(&req.password)?;
            hash_password(&req.password)?
        },
        role: Role::User, // nunca se acepta el rol desde el cliente
    };
    diesel::insert_into(users::table)
        .values(&new_user)
        .returning(User::as_returning())
        .get_result(conn)
        .map(UserResponse::from)
        .map_err(|e| match e {
            DieselError::DatabaseError(DatabaseErrorKind::UniqueViolation, _) => {
                ApiError::conflict("El usuario o el correo ya existe")
            }
            other => other.into(),
        })
}

/// Busca por username o email (el login acepta ambos).
pub fn find_by_login(conn: &mut PgConnection, login: &str) -> ApiResult<Option<User>> {
    let login = login.trim();
    Ok(users::table
        .filter(users::username.eq(login).or(users::email.eq(login.to_lowercase())))
        .select(User::as_select())
        .first(conn)
        .optional()?)
}

pub fn get_user(conn: &mut PgConnection, id: Uuid) -> ApiResult<User> {
    users::table.find(id).select(User::as_select()).first(conn).or_not_found("Usuario")
}

pub fn change_password(conn: &mut PgConnection, id: Uuid, req: ChangePasswordRequest) -> ApiResult<()> {
    if req.new_password != req.confirm_new_password {
        return Err(ApiError::bad_request("Las contraseñas nuevas no coinciden"));
    }
    validate_password(&req.new_password)?;
    let user = get_user(conn, id)?;
    if !bcrypt::verify(&req.old_password, &user.password_hash).unwrap_or(false) {
        return Err(ApiError::unauthorized("La contraseña actual es incorrecta"));
    }
    let new_hash = hash_password(&req.new_password)?;
    diesel::update(users::table.find(id))
        .set((users::password_hash.eq(new_hash), users::updated_at.eq(now())))
        .execute(conn)?;
    Ok(())
}

pub fn list_users(conn: &mut PgConnection, filter: UserFilter, p: Pagination) -> ApiResult<Paginated<UserResponse>> {
    let build = || {
        let mut q = users::table.into_boxed();
        if let Some(text) = filter.q.as_deref().filter(|s| !s.trim().is_empty()) {
            let pattern = format!("%{}%", text.trim());
            q = q.filter(users::username.ilike(pattern.clone()).or(users::email.ilike(pattern)));
        }
        if let Some(role) = filter.role {
            q = q.filter(users::role.eq(role));
        }
        q
    };
    let total: i64 = build().count().get_result(conn)?;
    let items = build()
        .order(users::created_at.desc())
        .limit(p.per_page())
        .offset(p.offset())
        .select(User::as_select())
        .load(conn)?
        .into_iter()
        .map(UserResponse::from)
        .collect();
    Ok(Paginated::new(items, &p, total))
}

pub fn set_role(conn: &mut PgConnection, id: Uuid, role: Role) -> ApiResult<UserResponse> {
    diesel::update(users::table.find(id))
        .set((users::role.eq(role), users::updated_at.eq(now())))
        .returning(User::as_returning())
        .get_result(conn)
        .or_not_found("Usuario")
        .map(UserResponse::from)
}

pub fn delete_user(conn: &mut PgConnection, id: Uuid) -> ApiResult<()> {
    let n = diesel::delete(users::table.find(id)).execute(conn)?;
    if n == 0 {
        return Err(ApiError::not_found("Usuario no encontrado"));
    }
    Ok(())
}

// ================= Recuperación de contraseña =================

use crate::models::tienda::NewPasswordReset;
use crate::schema::password_resets;
use sha2::{Digest, Sha256};

const RESET_TTL_MINUTES: i64 = 60;

fn hash_token(token: &str) -> String {
    Sha256::digest(token.as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}

/// Crea un token de recuperación. Devuelve `None` si el email no existe
/// (el endpoint responde igual en ambos casos para no revelar cuentas).
pub fn create_reset_token(conn: &mut PgConnection, email: &str) -> ApiResult<Option<(User, String)>> {
    let email = email.trim().to_lowercase();
    let Some(user) = users::table
        .filter(users::email.eq(&email))
        .select(User::as_select())
        .first(conn)
        .optional()?
    else {
        return Ok(None);
    };
    // 244 bits de aleatoriedad (dos UUIDv4)
    let token = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
    conn.transaction(|conn| {
        // Invalida tokens anteriores sin usar
        diesel::update(password_resets::table.filter(password_resets::user_id.eq(user.id).and(password_resets::used_at.is_null())))
            .set(password_resets::used_at.eq(now()))
            .execute(conn)?;
        diesel::insert_into(password_resets::table)
            .values(NewPasswordReset {
                user_id: user.id,
                token_hash: hash_token(&token),
                expires_at: now() + chrono::Duration::minutes(RESET_TTL_MINUTES),
            })
            .execute(conn)?;
        Ok::<_, ApiError>(())
    })?;
    Ok(Some((user, token)))
}

pub fn reset_password(conn: &mut PgConnection, token: &str, new_password: &str) -> ApiResult<()> {
    validate_password(new_password)?;
    conn.transaction(|conn| {
        let row: Option<(Uuid, Uuid)> = password_resets::table
            .filter(password_resets::token_hash.eq(hash_token(token.trim())))
            .filter(password_resets::used_at.is_null())
            .filter(password_resets::expires_at.gt(now()))
            .select((password_resets::id, password_resets::user_id))
            .for_update()
            .first(conn)
            .optional()?;
        let (reset_id, user_id) = row.ok_or_else(|| ApiError::bad_request("El enlace no es válido o ya expiró. Solicita uno nuevo"))?;
        diesel::update(users::table.find(user_id))
            .set((users::password_hash.eq(hash_password(new_password)?), users::updated_at.eq(now())))
            .execute(conn)?;
        diesel::update(password_resets::table.find(reset_id))
            .set(password_resets::used_at.eq(now()))
            .execute(conn)?;
        Ok(())
    })
}

pub const RESET_TTL: i64 = RESET_TTL_MINUTES;

// ----------------------------- 2FA (TOTP) -----------------------------

/// Genera el secreto 2FA (sin activar). Devuelve (secreto, url_otpauth, qr_png_base64).
pub fn twofa_setup(conn: &mut PgConnection, id: Uuid) -> ApiResult<(String, String, String)> {
    let u = get_user(conn, id)?;
    let secret = crate::twofa::generar_secret();
    let url = crate::twofa::url(&secret, &u.email)?;
    let qr = crate::twofa::qr(&secret, &u.email)?;
    diesel::update(users::table.find(id))
        .set((users::twofa_secret.eq(Some(crate::crypto::cifrar(&secret))), users::twofa_enabled.eq(false), users::updated_at.eq(now())))
        .execute(conn)?;
    Ok((secret, url, qr))
}

pub fn twofa_enable(conn: &mut PgConnection, id: Uuid, codigo: &str) -> ApiResult<()> {
    let u = get_user(conn, id)?;
    let secret = crate::crypto::descifrar(&u.twofa_secret.ok_or_else(|| ApiError::bad_request("Primero genera el 2FA"))?);
    if !crate::twofa::verificar(&secret, codigo) {
        return Err(ApiError::bad_request("Código de verificación inválido"));
    }
    diesel::update(users::table.find(id))
        .set((users::twofa_enabled.eq(true), users::updated_at.eq(now())))
        .execute(conn)?;
    Ok(())
}

pub fn twofa_disable(conn: &mut PgConnection, id: Uuid, codigo: &str) -> ApiResult<()> {
    let u = get_user(conn, id)?;
    if u.twofa_enabled && !crate::twofa::verificar(&crate::crypto::descifrar(u.twofa_secret.as_deref().unwrap_or("")), codigo) {
        return Err(ApiError::bad_request("Código de verificación inválido"));
    }
    diesel::update(users::table.find(id))
        .set((users::twofa_secret.eq::<Option<String>>(None), users::twofa_enabled.eq(false), users::updated_at.eq(now())))
        .execute(conn)?;
    Ok(())
}
