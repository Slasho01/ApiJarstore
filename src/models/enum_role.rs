use diesel::deserialize::{self, FromSql};
use diesel::pg::{Pg, PgValue};
use diesel::serialize::{self, Output, ToSql};
use diesel::sql_types::Text;
use diesel::{AsExpression, FromSqlRow};
use serde::{Deserialize, Serialize};
use std::io::Write;

/// Rol del usuario. En la BD y en JSON se guarda como "admin" | "seller" | "user".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, AsExpression, FromSqlRow)]
#[diesel(sql_type = Text)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Admin,
    Seller,
    #[default]
    User,
}

impl Role {
    pub fn as_str(&self) -> &'static str {
        match self {
            Role::Admin => "admin",
            Role::Seller => "seller",
            Role::User => "user",
        }
    }
}

impl ToSql<Text, Pg> for Role {
    fn to_sql<'b>(&'b self, out: &mut Output<'b, '_, Pg>) -> serialize::Result {
        out.write_all(self.as_str().as_bytes())?;
        Ok(serialize::IsNull::No)
    }
}

impl FromSql<Text, Pg> for Role {
    fn from_sql(bytes: PgValue<'_>) -> deserialize::Result<Self> {
        match std::str::from_utf8(bytes.as_bytes())? {
            "admin" => Ok(Role::Admin),
            "seller" => Ok(Role::Seller),
            "user" | "buyer" => Ok(Role::User),
            other => Err(format!("Rol no reconocido: {other}").into()),
        }
    }
}
