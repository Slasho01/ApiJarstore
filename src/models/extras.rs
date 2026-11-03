//! Reseñas, wishlist, direcciones y cupones.
use crate::schema::{cupones, direcciones, resenas, wishlist};
use chrono::NaiveDateTime;
use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Queryable, Selectable, Serialize, Debug)]
#[diesel(table_name = resenas, check_for_backend(diesel::pg::Pg))]
pub struct Resena {
    pub id: Uuid,
    pub user_id: Option<Uuid>,
    pub producto_id: Option<Uuid>,
    pub calificacion: i32,
    pub comentario: Option<String>,
    pub created_at: NaiveDateTime,
}

#[derive(Insertable, Debug)]
#[diesel(table_name = resenas)]
pub struct NewResena {
    pub user_id: Option<Uuid>,
    pub producto_id: Option<Uuid>,
    pub calificacion: i32,
    pub comentario: Option<String>,
}

#[derive(AsChangeset, Deserialize, Debug)]
#[diesel(table_name = resenas)]
pub struct ResenaUpdate {
    pub calificacion: Option<i32>,
    pub comentario: Option<String>,
}

#[derive(Queryable, Selectable, Serialize, Debug)]
#[diesel(table_name = wishlist, check_for_backend(diesel::pg::Pg))]
pub struct WishlistItem {
    pub id: Uuid,
    pub user_id: Uuid,
    pub producto_id: Uuid,
    pub created_at: NaiveDateTime,
}

#[derive(Queryable, Selectable, Serialize, Debug)]
#[diesel(table_name = direcciones, check_for_backend(diesel::pg::Pg))]
pub struct Direccion {
    pub id: Uuid,
    pub user_id: Uuid,
    pub titulo: Option<String>,
    pub direccion: String,
    pub ciudad: Option<String>,
    pub estado: Option<String>,
    pub codigo_postal: Option<String>,
    pub pais: Option<String>,
    pub telefono: Option<String>,
    pub created_at: NaiveDateTime,
}

impl Direccion {
    /// Texto en una línea para guardar en la orden (copia histórica).
    pub fn formatted(&self) -> String {
        [Some(self.direccion.as_str()), self.ciudad.as_deref(), self.estado.as_deref(),
         self.codigo_postal.as_deref(), self.pais.as_deref()]
            .into_iter()
            .flatten()
            .filter(|s| !s.trim().is_empty())
            .collect::<Vec<_>>()
            .join(", ")
    }
}

#[derive(Insertable, AsChangeset, Deserialize, Debug)]
#[diesel(table_name = direcciones)]
pub struct DireccionData {
    #[serde(skip)]
    pub user_id: Uuid,
    pub titulo: Option<String>,
    pub direccion: String,
    pub ciudad: Option<String>,
    pub estado: Option<String>,
    pub codigo_postal: Option<String>,
    pub pais: Option<String>,
    pub telefono: Option<String>,
}

#[derive(Queryable, Selectable, Serialize, Debug, Clone)]
#[diesel(table_name = cupones, check_for_backend(diesel::pg::Pg))]
pub struct Cupon {
    pub id: Uuid,
    pub codigo: String,
    pub descuento_porcentaje: Option<i32>,
    pub descuento_fijo: Option<i64>,
    pub fecha_expiracion: Option<NaiveDateTime>,
    pub uso_maximo: Option<i32>,
    pub uso_actual: i32,
    pub activo: bool,
    pub created_at: NaiveDateTime,
}

impl Cupon {
    /// Comprueba vigencia y calcula el descuento sobre un subtotal.
    pub fn descuento_para(&self, subtotal: i64, now: NaiveDateTime) -> Result<i64, String> {
        if !self.activo {
            return Err("El cupón no está activo".into());
        }
        if matches!(self.fecha_expiracion, Some(exp) if exp < now) {
            return Err("El cupón ha expirado".into());
        }
        if matches!(self.uso_maximo, Some(max) if self.uso_actual >= max) {
            return Err("El cupón alcanzó su límite de usos".into());
        }
        let descuento = match (self.descuento_porcentaje, self.descuento_fijo) {
            (Some(pct), _) => subtotal * pct as i64 / 100,
            (None, Some(fijo)) => fijo,
            (None, None) => 0,
        };
        Ok(descuento.clamp(0, subtotal))
    }
}

#[derive(Insertable, Deserialize, Debug)]
#[diesel(table_name = cupones)]
pub struct NewCupon {
    pub codigo: String,
    pub descuento_porcentaje: Option<i32>,
    pub descuento_fijo: Option<i64>,
    pub fecha_expiracion: Option<NaiveDateTime>,
    pub uso_maximo: Option<i32>,
    #[serde(default = "crate::models::category::default_true")]
    pub activo: bool,
}

#[derive(AsChangeset, Deserialize, Debug)]
#[diesel(table_name = cupones)]
pub struct CuponUpdate {
    pub descuento_porcentaje: Option<i32>,
    pub descuento_fijo: Option<i64>,
    pub fecha_expiracion: Option<NaiveDateTime>,
    pub uso_maximo: Option<i32>,
    pub activo: Option<bool>,
}
