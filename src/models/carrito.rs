use crate::schema::carrito;
use chrono::NaiveDateTime;
use diesel::prelude::*;
use serde::Serialize;
use uuid::Uuid;

#[derive(Queryable, Selectable, Serialize, Debug, Clone)]
#[diesel(table_name = carrito, check_for_backend(diesel::pg::Pg))]
pub struct CarritoItem {
    pub id: Uuid,
    pub user_id: Uuid,
    pub producto_id: Uuid,
    pub variante_id: Option<Uuid>,
    pub cantidad: i32,
    pub created_at: NaiveDateTime,
}

#[derive(Insertable, Debug)]
#[diesel(table_name = carrito)]
pub struct NewCarritoItem {
    pub user_id: Uuid,
    pub producto_id: Uuid,
    pub variante_id: Option<Uuid>,
    pub cantidad: i32,
}
