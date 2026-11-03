use crate::schema::categorias;
use chrono::NaiveDateTime;
use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Queryable, Selectable, Serialize, Debug, Clone)]
#[diesel(table_name = categorias, check_for_backend(diesel::pg::Pg))]
pub struct Categoria {
    pub id: Uuid,
    pub parent_id: Option<Uuid>,
    pub slug: String,
    pub nombre: String,
    pub descripcion: Option<String>,
    pub icono: Option<String>,
    pub orden: i32,
    pub estatus: bool,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}

#[derive(Insertable, Deserialize, Debug)]
#[diesel(table_name = categorias)]
pub struct NewCategoria {
    pub nombre: String,
    pub descripcion: Option<String>,
    #[serde(default = "default_true")]
    pub estatus: bool,
    pub parent_id: Option<Uuid>,
    /// Si no se envía, se genera a partir del nombre
    #[serde(default)]
    pub slug: String,
    #[serde(default)]
    pub orden: i32,
    pub icono: Option<String>,
}

#[derive(AsChangeset, Deserialize, Debug)]
#[diesel(table_name = categorias)]
pub struct CategoriaUpdate {
    pub nombre: Option<String>,
    pub descripcion: Option<String>,
    pub estatus: Option<bool>,
    pub slug: Option<String>,
    pub orden: Option<i32>,
    pub icono: Option<String>,
    /// `parent_id` se maneja aparte (admite null para dejarla en la raíz)
    #[serde(skip)]
    pub updated_at: Option<NaiveDateTime>,
}

pub fn default_true() -> bool {
    true
}
