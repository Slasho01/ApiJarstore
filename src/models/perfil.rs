use crate::schema::perfil;
use chrono::NaiveDateTime;
use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Queryable, Selectable, Serialize, Debug)]
#[diesel(table_name = perfil, check_for_backend(diesel::pg::Pg))]
pub struct Perfil {
    pub id: Uuid,
    pub user_id: Option<Uuid>,
    pub nombre: String,
    pub apellido: String,
    pub telefono: Option<String>,
    pub direccion: Option<String>,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}

#[derive(Insertable, Debug)]
#[diesel(table_name = perfil)]
pub struct NewPerfil {
    pub user_id: Option<Uuid>,
    pub nombre: String,
    pub apellido: String,
    pub telefono: Option<String>,
    pub direccion: Option<String>,
}

#[derive(AsChangeset, Deserialize, Debug)]
#[diesel(table_name = perfil)]
pub struct PerfilUpdate {
    pub nombre: Option<String>,
    pub apellido: Option<String>,
    pub telefono: Option<String>,
    pub direccion: Option<String>,
    #[serde(skip)]
    pub updated_at: Option<NaiveDateTime>,
}
