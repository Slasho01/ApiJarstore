use crate::errors::ApiError;
use crate::schema::{detalle_orden, envios, ordenes, pagos};
use chrono::NaiveDateTime;
use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Ciclo de vida de una orden:
/// pendiente → pagado → enviado → entregado
/// pendiente | pagado → cancelado (se devuelve el stock)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EstatusOrden {
    Pendiente,
    Pagado,
    Enviado,
    Entregado,
    Cancelado,
}

impl EstatusOrden {
    pub fn as_str(&self) -> &'static str {
        match self {
            EstatusOrden::Pendiente => "pendiente",
            EstatusOrden::Pagado => "pagado",
            EstatusOrden::Enviado => "enviado",
            EstatusOrden::Entregado => "entregado",
            EstatusOrden::Cancelado => "cancelado",
        }
    }

    pub fn parse(s: &str) -> Result<Self, ApiError> {
        match s {
            "pendiente" => Ok(EstatusOrden::Pendiente),
            "pagado" => Ok(EstatusOrden::Pagado),
            "enviado" => Ok(EstatusOrden::Enviado),
            "entregado" => Ok(EstatusOrden::Entregado),
            "cancelado" => Ok(EstatusOrden::Cancelado),
            other => Err(ApiError::internal(format!("Estatus de orden desconocido en BD: {other}"))),
        }
    }

    pub fn can_transition_to(&self, next: EstatusOrden) -> bool {
        use EstatusOrden::*;
        matches!(
            (self, next),
            (Pendiente, Pagado)
                | (Pagado, Enviado)
                | (Enviado, Entregado)
                | (Pendiente, Cancelado)
                | (Pagado, Cancelado)
        )
    }
}

#[derive(Queryable, Selectable, Identifiable, Serialize, Debug, Clone)]
#[diesel(table_name = ordenes, check_for_backend(diesel::pg::Pg))]
pub struct Orden {
    pub id: Uuid,
    pub user_id: Option<Uuid>,
    pub subtotal: i64,
    pub descuento: i64,
    pub total: i64,
    pub cupon_id: Option<Uuid>,
    pub estatus: String,
    pub metodo_pago: Option<String>,
    pub direccion_envio: Option<String>,
    pub costo_envio: i64,
    pub metodo_envio: Option<String>,
    pub pais: Option<String>,
    pub moneda: String,
    pub tasa_cambio: f64,
    pub nombre_comprador: Option<String>,
    pub email_comprador: Option<String>,
    pub telefono_comprador: Option<String>,
    /// Proveedor de pago online elegido (p. ej. "mercadopago"); None = pago manual
    pub pago_proveedor: Option<String>,
    /// Puntos canjeados en este pedido y su equivalente en dinero (moneda base)
    pub puntos_usados: i32,
    pub descuento_puntos: i64,
    /// Puntos acreditados al pagarse
    pub puntos_ganados: i32,
    /// Documento tributario: "boleta" | "factura" | None (sólo Chile)
    pub tipo_documento: Option<String>,
    pub factura_rut: Option<String>,
    pub factura_razon_social: Option<String>,
    pub factura_giro: Option<String>,
    pub factura_direccion: Option<String>,
    /// Documento emitido (PDF o imagen) que la tienda sube al pedido
    pub documento_url: Option<String>,
    pub documento_subido_at: Option<NaiveDateTime>,
    pub comuna_destino: Option<String>,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}

#[derive(Insertable, Debug)]
#[diesel(table_name = ordenes)]
pub struct NewOrden {
    pub user_id: Option<Uuid>,
    pub subtotal: i64,
    pub descuento: i64,
    pub total: i64,
    pub cupon_id: Option<Uuid>,
    pub estatus: String,
    pub metodo_pago: Option<String>,
    pub direccion_envio: Option<String>,
    pub costo_envio: i64,
    pub metodo_envio: Option<String>,
    pub pais: Option<String>,
    pub moneda: String,
    pub tasa_cambio: f64,
    pub nombre_comprador: Option<String>,
    pub email_comprador: Option<String>,
    pub telefono_comprador: Option<String>,
    pub pago_proveedor: Option<String>,
    pub puntos_usados: i32,
    pub descuento_puntos: i64,
    pub tipo_documento: Option<String>,
    pub factura_rut: Option<String>,
    pub factura_razon_social: Option<String>,
    pub factura_giro: Option<String>,
    pub factura_direccion: Option<String>,
    pub comuna_destino: Option<String>,
}

#[derive(Queryable, Selectable, Serialize, Debug)]
#[diesel(table_name = detalle_orden, check_for_backend(diesel::pg::Pg))]
pub struct DetalleOrden {
    pub id: Uuid,
    pub orden_id: Option<Uuid>,
    pub producto_id: Option<Uuid>,
    pub variante_id: Option<Uuid>,
    pub nombre_producto: String,
    pub nombre_variante: Option<String>,
    pub cantidad: i32,
    pub precio_unitario: i64,
}

#[derive(Insertable, Debug)]
#[diesel(table_name = detalle_orden)]
pub struct NewDetalleOrden {
    pub orden_id: Option<Uuid>,
    pub producto_id: Option<Uuid>,
    pub variante_id: Option<Uuid>,
    pub nombre_producto: String,
    pub nombre_variante: Option<String>,
    pub cantidad: i32,
    pub precio_unitario: i64,
}

#[derive(Queryable, Selectable, Serialize, Debug)]
#[diesel(table_name = pagos, check_for_backend(diesel::pg::Pg))]
pub struct Pago {
    pub id: Uuid,
    pub orden_id: Option<Uuid>,
    pub monto: i64,
    pub metodo: Option<String>,
    pub estatus: Option<String>,
    pub referencia_externa: Option<String>,
    pub proveedor: Option<String>,
    pub created_at: NaiveDateTime,
}

#[derive(Insertable, Debug)]
#[diesel(table_name = pagos)]
pub struct NewPago {
    pub orden_id: Option<Uuid>,
    pub monto: i64,
    pub metodo: Option<String>,
    pub estatus: Option<String>,
    pub referencia_externa: Option<String>,
    pub proveedor: Option<String>,
}

#[derive(Queryable, Selectable, Serialize, Debug)]
#[diesel(table_name = envios, check_for_backend(diesel::pg::Pg))]
pub struct Envio {
    pub id: Uuid,
    pub orden_id: Option<Uuid>,
    pub empresa_envio: Option<String>,
    pub codigo_seguimiento: Option<String>,
    pub estatus: Option<String>,
    pub fecha_envio: Option<NaiveDateTime>,
    pub fecha_entrega: Option<NaiveDateTime>,
}

#[derive(Insertable, AsChangeset, Deserialize, Debug)]
#[diesel(table_name = envios)]
pub struct EnvioData {
    #[serde(skip)]
    pub orden_id: Option<Uuid>,
    pub empresa_envio: Option<String>,
    pub codigo_seguimiento: Option<String>,
    pub estatus: Option<String>,
    pub fecha_envio: Option<NaiveDateTime>,
    pub fecha_entrega: Option<NaiveDateTime>,
}
