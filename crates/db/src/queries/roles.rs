//! Reading a relay's roles.

use sqlx::{PgConnection, PgExecutor};

use caravel_domain::error::{Error, Result};
use caravel_domain::roles::Role;

pub async fn list_roles(db: impl PgExecutor<'_>, relay_id: &str) -> Result<Vec<Role>> {
    Ok(sqlx::query_as::<_, Role>(
        "SELECT id, label, description, hue, position FROM role WHERE relay_id = $1 ORDER BY position, id",
    )
    .bind(relay_id)
    .fetch_all(db)
    .await?)
}

pub async fn require_role(conn: &mut PgConnection, relay_id: &str, role_id: &str) -> Result<()> {
    let exists = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (SELECT 1 FROM role WHERE relay_id = $1 AND id = $2)",
    )
    .bind(relay_id)
    .bind(role_id)
    .fetch_one(conn)
    .await?;

    if exists {
        Ok(())
    } else {
        Err(Error::conflict(
            "role-missing",
            format!("role {role_id:?} does not exist"),
        ))
    }
}
