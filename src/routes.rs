use crate::controller::*;
use crate::db::{self, DbPool};
use crate::errors::ApiResult;
use actix_web::{web, HttpResponse};
use diesel::prelude::*;
use serde_json::json;

async fn health(pool: web::Data<DbPool>) -> ApiResult<HttpResponse> {
    db::run(&pool, |conn| {
        diesel::sql_query("SELECT 1").execute(conn)?;
        Ok(())
    })
    .await?;
    Ok(HttpResponse::Ok().json(json!({ "status": "ok", "version": env!("CARGO_PKG_VERSION") })))
}

/// Mapa de rutas. Las rutas protegidas lo son porque su handler pide `AuthUser`;
/// los permisos por rol se comprueban dentro de cada handler.
pub fn configure_routes(cfg: &mut web::ServiceConfig) {
    use actix_web::web::{delete, get, post, put, resource as r};

    cfg.service(
        web::scope("/api/v2")
            .service(r("/health").route(get().to(health)))
            // ---- Auth ----
            .service(r("/auth/login").route(post().to(auth_controller::login)))
            .service(r("/auth/register").route(post().to(auth_controller::register)))
            .service(r("/auth/forgot").route(post().to(auth_controller::forgot_password)))
            .service(r("/auth/reset").route(post().to(auth_controller::reset_password)))
            .service(r("/login").route(post().to(auth_controller::login))) // compatibilidad
            // ---- Tienda (configuración pública y páginas) ----
            .service(r("/config").route(get().to(tienda_controller::public_config)))
            .service(r("/paginas/{slug}").route(get().to(tienda_controller::page)))
            // ---- Usuarios ----
            .service(r("/users").route(post().to(auth_controller::register))) // compatibilidad
            .service(r("/users/me").route(get().to(user_controller::me)))
            .service(r("/users/me/password").route(put().to(user_controller::change_my_password)))
            .service(
                r("/users/{id}")
                    .route(get().to(user_controller::get_by_id))
                    .route(put().to(user_controller::change_password_by_id)),
            )
            // ---- Perfil ----
            .service(r("/perfil").route(post().to(perfil_controller::create)))
            .service(
                r("/perfil/mi-perfil")
                    .route(get().to(perfil_controller::mine))
                    .route(put().to(perfil_controller::update_mine)),
            )
            // ---- Categorías ----
            .service(
                r("/categorias")
                    .route(get().to(category_controller::list))
                    .route(post().to(category_controller::create)),
            )
            .service(
                r("/categorias/{id}")
                    .route(get().to(category_controller::get))
                    .route(put().to(category_controller::update))
                    .route(delete().to(category_controller::delete)),
            )
            // ---- Productos ----
            .service(
                r("/productos")
                    .route(get().to(product_controller::list))
                    .route(post().to(product_controller::create)),
            )
            .service(r("/productos/mios").route(get().to(product_controller::mine)))
            .service(r("/marcas/{slug}").route(get().to(product_controller::brand)))
            .service(r("/buscar/sugerencias").route(get().to(product_controller::suggestions)))
            .service(r("/uploads").route(post().to(upload_controller::upload)))
            .service(r("/banners").route(get().to(marketing_controller::banners)))
            .service(r("/newsletter").route(post().to(marketing_controller::subscribe)))
            .service(r("/newsletter/confirmar").route(post().to(marketing_controller::confirm)))
            .service(r("/newsletter/baja").route(post().to(marketing_controller::unsubscribe)))
            .service(r("/pagos/mercadopago/webhook").route(post().to(order_controller::mp_webhook)))
            .service(r("/productos/facetas").route(get().to(product_controller::facets)))
            .service(r("/productos/mas-vendidos").route(get().to(product_controller::best_sellers)))
            .service(
                r("/productos/{id}")
                    .route(get().to(product_controller::detail))
                    .route(put().to(product_controller::update))
                    .route(delete().to(product_controller::delete)),
            )
            .service(r("/productos/{id}/relacionados").route(get().to(product_controller::related)))
            .service(r("/productos/{id}/avisar-stock").route(post().to(notificaciones_controller::subscribe_stock)))
            .service(
                r("/productos/{id}/complementos")
                    .route(get().to(product_controller::complements))
                    .route(put().to(product_controller::set_complements)),
            )
            .service(r("/productos/{id}/variantes").route(post().to(product_controller::add_variant)))
            .service(r("/productos/{id}/imagenes").route(post().to(product_controller::add_product_image)))
            .service(
                r("/productos/{id}/resenas")
                    .route(get().to(extras_controller::reviews_list))
                    .route(post().to(extras_controller::review_create)),
            )
            .service(r("/imagenes/{id}").route(delete().to(product_controller::delete_product_image)))
            .service(r("/variantes/imagenes/{id}").route(delete().to(product_controller::delete_variant_image)))
            .service(
                r("/variantes/{id}")
                    .route(put().to(product_controller::update_variant))
                    .route(delete().to(product_controller::delete_variant)),
            )
            .service(r("/variantes/{id}/imagenes").route(post().to(product_controller::add_variant_image)))
            // ---- Reseñas ----
            .service(
                r("/resenas/{id}")
                    .route(put().to(extras_controller::review_update))
                    .route(delete().to(extras_controller::review_delete)),
            )
            // ---- Carrito ----
            .service(
                r("/carrito")
                    .route(get().to(cart_controller::view))
                    .route(post().to(cart_controller::add))
                    .route(delete().to(cart_controller::clear)),
            )
            .service(
                r("/carrito/{id}")
                    .route(put().to(cart_controller::update_quantity))
                    .route(delete().to(cart_controller::remove)),
            )
            // ---- Órdenes ----
            .service(
                r("/ordenes")
                    .route(get().to(order_controller::mine))
                    .route(post().to(order_controller::checkout)),
            )
            .service(r("/puntos").route(get().to(puntos_controller::mine)))
            .service(r("/ordenes/cotizar").route(post().to(order_controller::quote)))
            .service(r("/ordenes/{id}").route(get().to(order_controller::get)))
            .service(r("/ordenes/{id}/cancelar").route(post().to(order_controller::cancel)))
            .service(r("/ordenes/{id}/pagar").route(post().to(order_controller::pay)))
            .service(r("/ordenes/{id}/pago/verificar").route(post().to(order_controller::verify_payment)))
            // ---- Wishlist ----
            .service(
                r("/wishlist")
                    .route(get().to(extras_controller::wishlist))
                    .route(post().to(extras_controller::wishlist_add)),
            )
            .service(r("/wishlist/{producto_id}").route(delete().to(extras_controller::wishlist_remove)))
            // ---- Direcciones ----
            .service(
                r("/direcciones")
                    .route(get().to(extras_controller::addresses))
                    .route(post().to(extras_controller::address_create)),
            )
            .service(
                r("/direcciones/{id}")
                    .route(put().to(extras_controller::address_update))
                    .route(delete().to(extras_controller::address_delete)),
            )
            // ---- Cupones ----
            .service(r("/cupones/{codigo}/validar").route(get().to(extras_controller::coupon_check)))
            // ---- Admin ----
            .service(
                web::scope("/admin")
                    .service(r("/users").route(get().to(user_controller::admin_list)))
                    .service(r("/users/{id}").route(delete().to(user_controller::admin_delete)))
                    .service(r("/users/{id}/role").route(put().to(user_controller::admin_set_role)))
                    .service(
                        r("/users/{id}/puntos")
                            .route(get().to(puntos_controller::admin_get))
                            .route(post().to(puntos_controller::admin_adjust)),
                    )
                    .service(r("/metricas").route(get().to(tienda_controller::admin_metrics)))
                    .service(r("/stock-bajo").route(get().to(notificaciones_controller::admin_low_stock)))
                    .service(r("/notificaciones/procesar").route(post().to(notificaciones_controller::admin_process)))
                    .service(r("/config").route(get().to(tienda_controller::admin_config)))
                    .service(
                        r("/banners")
                            .route(get().to(marketing_controller::admin_banners))
                            .route(post().to(marketing_controller::admin_banner_create)),
                    )
                    .service(
                        r("/banners/{id}")
                            .route(put().to(marketing_controller::admin_banner_update))
                            .route(delete().to(marketing_controller::admin_banner_delete)),
                    )
                    .service(r("/newsletter").route(get().to(marketing_controller::admin_subscribers)))
                    .service(r("/newsletter.csv").route(get().to(marketing_controller::admin_subscribers_csv)))
                    .service(r("/config/smtp/test").route(post().to(tienda_controller::admin_smtp_test)))
                    .service(r("/config/{clave}").route(put().to(tienda_controller::admin_set_config)))
                    .service(
                        r("/paginas")
                            .route(get().to(tienda_controller::admin_pages))
                            .route(post().to(tienda_controller::admin_page_create)),
                    )
                    .service(
                        r("/paginas/{id}")
                            .route(put().to(tienda_controller::admin_page_update))
                            .route(delete().to(tienda_controller::admin_page_delete)),
                    )
                    .service(r("/perfiles").route(get().to(perfil_controller::admin_list)))
                    .service(r("/perfiles/{user_id}").route(get().to(perfil_controller::admin_get)))
                    .service(r("/ordenes").route(get().to(order_controller::admin_list)))
                    .service(r("/ordenes/{id}/estatus").route(put().to(order_controller::admin_set_status)))
                    .service(r("/ordenes/{id}/pagos").route(post().to(order_controller::admin_add_payment)))
                    .service(r("/ordenes/{id}/envio").route(put().to(order_controller::admin_set_shipping)))
                    .service(r("/ordenes/{id}/documento").route(post().to(order_controller::admin_set_document)))
                    .service(
                        r("/cupones")
                            .route(get().to(extras_controller::coupons_list))
                            .route(post().to(extras_controller::coupon_create)),
                    )
                    .service(
                        r("/cupones/{id}")
                            .route(put().to(extras_controller::coupon_update))
                            .route(delete().to(extras_controller::coupon_delete)),
                    )
                    // ---- MeliHub (Mercado Libre) ----
                    .service(r("/melihub/estado").route(get().to(melihub_controller::admin_status)))
                    .service(r("/melihub/sincronizar").route(post().to(melihub_controller::admin_sync))),
            ),
    );
}
