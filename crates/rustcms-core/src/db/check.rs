//! MySQL setup check: collation, engine, `sql_mode`, connection settings.

use super::*;

/// Refuses a MySQL database whose settings would make rustcms behave unlike on
/// PostgreSQL: string columns (and the database default) must use
/// [`MYSQL_COLLATION`], tables must be InnoDB (transactions, row locks), and
/// `sql_mode` must be strict (without `NO_BACKSLASH_ESCAPES`, which changes
/// how string literals read), globally and on this session. The connection must
/// use UTC and [`MYSQL_COLLATION`] (pools from [`connect`] always do; a pool
/// built elsewhere must too). The error lists the SQL that fixes it. Other
/// databases pass. With `tables = false` only the database itself is checked
/// (before `migrate:fresh` recreates every table).
pub async fn check(db: &impl ConnectionTrait, tables: bool) -> anyhow::Result<()> {
    if db.get_database_backend() != DbBackend::MySql {
        return Ok(());
    }
    let rows = |sql: &str| db.query_all_raw(Statement::from_string(DbBackend::MySql, sql.to_owned()));
    let mut problems = Vec::new();
    let settings = rows(
        "SELECT s.schema_name AS db, s.default_character_set_name AS charset, \
         s.default_collation_name AS collation, @@SESSION.sql_mode AS mode, @@GLOBAL.sql_mode AS global_mode, \
         @@SESSION.time_zone AS zone, @@SESSION.collation_connection AS connection \
         FROM information_schema.schemata s WHERE s.schema_name = DATABASE()",
    )
    .await?;
    let Some(settings) = settings.first() else { anyhow::bail!("MySQL: no database selected") };
    let name: String = settings.try_get("", "db")?;
    let collation: String = settings.try_get("", "collation")?;
    if settings.try_get::<String>("", "charset")? != "utf8mb4" || collation != MYSQL_COLLATION {
        problems.push(format!("ALTER DATABASE `{name}` CHARACTER SET utf8mb4 COLLATE {MYSQL_COLLATION};"));
    }
    for column in ["mode", "global_mode"] {
        let mode: String = settings.try_get("", column)?;
        let modes: Vec<&str> = mode.split(',').collect();
        if !modes.contains(&"STRICT_TRANS_TABLES") || modes.contains(&"NO_BACKSLASH_ESCAPES") {
            problems.push(
                "SET PERSIST sql_mode = '…'; -- with STRICT_TRANS_TABLES, without NO_BACKSLASH_ESCAPES (then reconnect)"
                    .into(),
            );
            break;
        }
    }
    let zone: String = settings.try_get("", "zone")?;
    let connection: String = settings.try_get("", "connection")?;
    if zone != "+00:00" || connection != MYSQL_COLLATION {
        problems.push(format!(
            "-- connections need time_zone '+00:00' and collation {MYSQL_COLLATION} (open the pool with rustcms_core::db::connect)"
        ));
    }
    if tables {
        let columns = rows(&format!(
            "SELECT DISTINCT table_name AS t FROM information_schema.columns \
             WHERE table_schema = DATABASE() AND collation_name IS NOT NULL AND collation_name <> '{MYSQL_COLLATION}' \
             ORDER BY t"
        ))
        .await?;
        for row in columns {
            let table: String = row.try_get("", "t")?;
            problems.push(format!("ALTER TABLE `{table}` CONVERT TO CHARACTER SET utf8mb4 COLLATE {MYSQL_COLLATION};"));
        }
        let engines = rows(
            "SELECT table_name AS t FROM information_schema.tables \
             WHERE table_schema = DATABASE() AND table_type = 'BASE TABLE' AND engine <> 'InnoDB' ORDER BY t",
        )
        .await?;
        for row in engines {
            let table: String = row.try_get("", "t")?;
            problems.push(format!("ALTER TABLE `{table}` ENGINE = InnoDB;"));
        }
    }
    if problems.is_empty() {
        return Ok(());
    }
    anyhow::bail!(
        "MySQL database `{name}` is not set up for rustcms (see the configuration guide). Run:\n  {}",
        problems.join("\n  ")
    )
}
