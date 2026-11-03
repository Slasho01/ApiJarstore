//! Handlers HTTP: validan rol/propiedad, delegan en `services` y devuelven JSON.
pub mod auth_controller;
pub mod cart_controller;
pub mod category_controller;
pub mod extras_controller;
pub mod marketing_controller;
pub mod notificaciones_controller;
pub mod order_controller;
pub mod perfil_controller;
pub mod product_controller;
pub mod puntos_controller;
pub mod tienda_controller;
pub mod upload_controller;
pub mod user_controller;

use serde_json::json;

pub fn ok_message(msg: impl Into<String>) -> actix_web::HttpResponse {
    actix_web::HttpResponse::Ok().json(json!({ "message": msg.into() }))
}
