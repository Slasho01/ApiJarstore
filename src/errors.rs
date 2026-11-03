use actix_web::{http::StatusCode, HttpResponse, ResponseError};
use diesel::result::{DatabaseErrorKind, Error as DieselError};
use serde_json::json;
use std::fmt;

/// Error único de la API. Todos los servicios y controladores devuelven
/// `Result<_, ApiError>` y actix lo convierte en una respuesta JSON:
/// `{ "error": "mensaje legible", "code": "CODIGO_MAQUINA" }`
#[derive(Debug)]
pub enum ApiError {
    BadRequest(String),
    Unauthorized(String),
    Forbidden(String),
    NotFound(String),
    Conflict(String),
    TooManyRequests(String),
    Internal(String),
}

pub type ApiResult<T> = Result<T, ApiError>;

impl ApiError {
    pub fn bad_request(msg: impl Into<String>) -> Self { ApiError::BadRequest(msg.into()) }
    pub fn not_found(msg: impl Into<String>) -> Self { ApiError::NotFound(msg.into()) }
    pub fn forbidden(msg: impl Into<String>) -> Self { ApiError::Forbidden(msg.into()) }
    pub fn conflict(msg: impl Into<String>) -> Self { ApiError::Conflict(msg.into()) }
    pub fn unauthorized(msg: impl Into<String>) -> Self { ApiError::Unauthorized(msg.into()) }
    pub fn internal(msg: impl Into<String>) -> Self { ApiError::Internal(msg.into()) }

    fn code(&self) -> &'static str {
        match self {
            ApiError::BadRequest(_) => "BAD_REQUEST",
            ApiError::Unauthorized(_) => "UNAUTHORIZED",
            ApiError::Forbidden(_) => "FORBIDDEN",
            ApiError::NotFound(_) => "NOT_FOUND",
            ApiError::Conflict(_) => "CONFLICT",
            ApiError::TooManyRequests(_) => "TOO_MANY_REQUESTS",
            ApiError::Internal(_) => "INTERNAL_ERROR",
        }
    }

    fn message(&self) -> &str {
        match self {
            ApiError::BadRequest(m)
            | ApiError::Unauthorized(m)
            | ApiError::Forbidden(m)
            | ApiError::NotFound(m)
            | ApiError::Conflict(m)
            | ApiError::TooManyRequests(m) => m,
            // Nunca se filtran detalles internos al cliente (se registran en el log).
            ApiError::Internal(_) => "Error interno del servidor",
        }
    }
}

impl fmt::Display for ApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ApiError::Internal(m) => write!(f, "{}: {}", self.code(), m),
            _ => write!(f, "{}: {}", self.code(), self.message()),
        }
    }
}

impl ResponseError for ApiError {
    fn status_code(&self) -> StatusCode {
        match self {
            ApiError::BadRequest(_) => StatusCode::BAD_REQUEST,
            ApiError::Unauthorized(_) => StatusCode::UNAUTHORIZED,
            ApiError::Forbidden(_) => StatusCode::FORBIDDEN,
            ApiError::NotFound(_) => StatusCode::NOT_FOUND,
            ApiError::Conflict(_) => StatusCode::CONFLICT,
            ApiError::TooManyRequests(_) => StatusCode::TOO_MANY_REQUESTS,
            ApiError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    fn error_response(&self) -> HttpResponse {
        if let ApiError::Internal(detail) = self {
            log::error!("{}", detail);
        }
        HttpResponse::build(self.status_code()).json(json!({
            "error": self.message(),
            "code": self.code(),
        }))
    }
}

impl From<DieselError> for ApiError {
    fn from(err: DieselError) -> Self {
        match err {
            DieselError::NotFound => ApiError::not_found("Recurso no encontrado"),
            DieselError::DatabaseError(DatabaseErrorKind::UniqueViolation, info) => {
                ApiError::conflict(format!("Ya existe un registro con esos datos ({})",
                    info.constraint_name().unwrap_or("único")))
            }
            DieselError::DatabaseError(DatabaseErrorKind::ForeignKeyViolation, info) => {
                ApiError::bad_request(format!("Referencia inválida ({})",
                    info.constraint_name().unwrap_or("llave foránea")))
            }
            DieselError::DatabaseError(DatabaseErrorKind::CheckViolation, info) => {
                ApiError::bad_request(format!("Valor fuera de rango ({})",
                    info.constraint_name().unwrap_or("check")))
            }
            other => ApiError::internal(format!("Error de base de datos: {other}")),
        }
    }
}

impl From<diesel::r2d2::PoolError> for ApiError {
    fn from(err: diesel::r2d2::PoolError) -> Self {
        ApiError::internal(format!("No se pudo obtener conexión del pool: {err}"))
    }
}

impl From<actix_web::error::BlockingError> for ApiError {
    fn from(err: actix_web::error::BlockingError) -> Self {
        ApiError::internal(format!("Error en hilo bloqueante: {err}"))
    }
}

/// Atajo para convertir `Option` en 404.
pub trait OrNotFound<T> {
    fn or_not_found(self, what: &str) -> ApiResult<T>;
}

impl<T> OrNotFound<T> for Result<T, DieselError> {
    fn or_not_found(self, what: &str) -> ApiResult<T> {
        match self {
            Ok(v) => Ok(v),
            Err(DieselError::NotFound) => Err(ApiError::not_found(format!("{what} no encontrado"))),
            Err(e) => Err(e.into()),
        }
    }
}
