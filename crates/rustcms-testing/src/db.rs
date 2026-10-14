use rustcms_core::config::DatabaseConfig;
use sea_orm::{ConnectOptions, ConnectionTrait, Database, DatabaseConnection, DbBackend, Statement};
use sea_orm_migration::MigratorTrait;
use uuid::Uuid;

/// Database engine of a [`TestDb`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    /// SQLite file in the temp directory.
    Sqlite,
    /// Dedicated PostgreSQL database.
    Postgres,
    /// Dedicated MySQL database (`utf8mb4_0900_bin`).
    Mysql,
}

/// An isolated, initially empty database owned by one test.
///
/// - SQLite: a temp file, opened with the same options as production
///   (foreign keys, WAL, busy timeout), so locking behaves like the real thing.
/// - PostgreSQL: a new database created on the server in `RUSTCMS_TEST_PG`.
///   No `search_path` tricks: every pooled connection sees the same database.
/// - MySQL: a new database (`utf8mb4_0900_bin`) on the server in `RUSTCMS_TEST_MYSQL`.
#[derive(Clone)]
pub struct TestDb {
    conn: DatabaseConnection,
    url: String,
    backend: Backend,
    admin_url: Option<String>,
    name: String,
}

const ENV: &str = "RUSTCMS_TEST_PG";
const MYSQL_ENV: &str = "RUSTCMS_TEST_MYSQL";
/// The collation production requires (see `rustcms_core::db`).
const MYSQL_DATABASE: &str = "CHARACTER SET utf8mb4 COLLATE utf8mb4_0900_bin";
/// Marks a finished MySQL template (MySQL cannot rename a database).
const READY: &str = "_rustcms_template_ready";
/// Advisory lock serializing template creation across test processes.
const LOCK: i64 = 0x7275_7374_636d_7301;

impl TestDb {
    /// A fresh SQLite database file.
    pub async fn sqlite() -> Self {
        let path = std::env::temp_dir().join(format!("rustcms-test-{}.db", Uuid::new_v4()));
        let url = format!("sqlite://{}?mode=rwc", path.display());
        let conn = connect(&url).await;
        Self { conn, url, backend: Backend::Sqlite, admin_url: None, name: path.display().to_string() }
    }

    /// A fresh empty database, or `None` when the server's variable
    /// (`RUSTCMS_TEST_PG`, `RUSTCMS_TEST_MYSQL`) is not set.
    pub async fn empty(backend: Backend) -> Option<Self> {
        match backend {
            Backend::Sqlite => Some(Self::sqlite().await),
            Backend::Postgres => Self::postgres().await,
            Backend::Mysql => Self::mysql().await,
        }
    }

    /// A fresh MySQL database, or `None` when `RUSTCMS_TEST_MYSQL` is not set.
    pub async fn mysql() -> Option<Self> {
        let admin_url = std::env::var(MYSQL_ENV).ok()?;
        let name = format!("rustcms_t_{}", Uuid::new_v4().simple());
        let admin = Database::connect(&admin_url).await.expect("connect to RUSTCMS_TEST_MYSQL");
        admin
            .execute_unprepared(&format!("CREATE DATABASE {name} {MYSQL_DATABASE}"))
            .await
            .expect("create test database");
        admin.close().await.ok();
        let url = replace_database(&admin_url, &name);
        let conn = connect(&url).await;
        Some(Self { conn, url, backend: Backend::Mysql, admin_url: Some(admin_url), name })
    }

    /// A fresh PostgreSQL database, or `None` when `RUSTCMS_TEST_PG` is not set.
    pub async fn postgres() -> Option<Self> {
        let admin_url = std::env::var(ENV).ok()?;
        let name = format!("rustcms_t_{}", Uuid::new_v4().simple());
        let admin = Database::connect(&admin_url).await.expect("connect to RUSTCMS_TEST_PG");
        admin.execute_unprepared(&format!("CREATE DATABASE {name}")).await.expect("create test database");
        admin.close().await.ok();
        let url = replace_database(&admin_url, &name);
        let conn = connect(&url).await;
        Some(Self { conn, url, backend: Backend::Postgres, admin_url: Some(admin_url), name })
    }

    /// A fresh database with every migration of `M` applied.
    ///
    /// On PostgreSQL the migrated schema is built once per set of migration
    /// names as a template database, then cloned for each test. Run
    /// `just test-reset-db` after editing an unreleased migration in place.
    pub async fn migrated<M: MigratorTrait>(backend: Backend) -> Option<Self> {
        match backend {
            Backend::Sqlite => {
                let db = Self::sqlite().await;
                db.migrate::<M>().await;
                Some(db)
            }
            Backend::Postgres => {
                let admin_url = std::env::var(ENV).ok()?;
                let template = format!("rustcms_tpl_{:016x}", fingerprint(M::migrations().iter().map(|m| m.name())));
                let name = format!("rustcms_t_{}", Uuid::new_v4().simple());
                let admin = admin_connection(&admin_url).await;
                admin.execute_unprepared(&format!("SELECT pg_advisory_lock({LOCK})")).await.expect("advisory lock");
                let exists = admin
                    .query_one_raw(Statement::from_sql_and_values(
                        DbBackend::Postgres,
                        "SELECT 1 FROM pg_database WHERE datname = $1",
                        [template.clone().into()],
                    ))
                    .await
                    .expect("query templates")
                    .is_some();
                if !exists {
                    let staging = format!("{template}_staging");
                    admin
                        .execute_unprepared(&format!("DROP DATABASE IF EXISTS {staging}"))
                        .await
                        .expect("drop staging");
                    admin.execute_unprepared(&format!("CREATE DATABASE {staging}")).await.expect("create template");
                    let conn = connect(&replace_database(&admin_url, &staging)).await;
                    M::up(&conn, None).await.expect("migrate template database");
                    conn.close().await.ok();
                    admin
                        .execute_unprepared(&format!("ALTER DATABASE {staging} RENAME TO {template}"))
                        .await
                        .expect("publish template");
                }
                admin
                    .execute_unprepared(&format!("CREATE DATABASE {name} TEMPLATE {template}"))
                    .await
                    .expect("clone template");
                admin.execute_unprepared(&format!("SELECT pg_advisory_unlock({LOCK})")).await.ok();
                admin.close().await.ok();
                let url = replace_database(&admin_url, &name);
                let conn = connect(&url).await;
                Some(Self { conn, url, backend, admin_url: Some(admin_url), name })
            }
            Backend::Mysql => Self::mysql_migrated::<M>().await,
        }
    }

    /// MySQL has no template databases: the migrated schema is built once per
    /// set of migrations, then each test database gets its tables (with their
    /// foreign keys, from `SHOW CREATE TABLE`) and rows.
    async fn mysql_migrated<M: MigratorTrait>() -> Option<Self> {
        let admin_url = std::env::var(MYSQL_ENV).ok()?;
        let template = format!("rustcms_tpl_{:016x}", fingerprint(M::migrations().iter().map(|m| m.name())));
        let name = format!("rustcms_t_{}", Uuid::new_v4().simple());
        let admin = admin_connection(&admin_url).await;
        let one =
            |sql: String, values: Vec<sea_orm::Value>| Statement::from_sql_and_values(DbBackend::MySql, sql, values);
        // A session lock: held by this single connection until released below.
        let lock = admin
            .query_one_raw(one("SELECT GET_LOCK('rustcms_template', 600) AS ok".into(), vec![]))
            .await
            .expect("template lock")
            .and_then(|row| row.try_get::<Option<i64>>("", "ok").ok().flatten());
        assert_eq!(lock, Some(1), "could not take the MySQL template lock");
        let ready = admin
            .query_one_raw(one(
                "SELECT 1 FROM information_schema.tables WHERE table_schema = ? AND table_name = ?".into(),
                vec![template.clone().into(), READY.into()],
            ))
            .await
            .expect("query templates")
            .is_some();
        if !ready {
            admin.execute_unprepared(&format!("DROP DATABASE IF EXISTS {template}")).await.expect("drop template");
            admin
                .execute_unprepared(&format!("CREATE DATABASE {template} {MYSQL_DATABASE}"))
                .await
                .expect("create template");
            let conn = connect(&replace_database(&admin_url, &template)).await;
            M::up(&conn, None).await.expect("migrate template database");
            conn.close().await.ok();
            let objects = admin
                .query_one_raw(one(
                    "SELECT (SELECT COUNT(*) FROM information_schema.views WHERE table_schema = ?) + \
                     (SELECT COUNT(*) FROM information_schema.triggers WHERE trigger_schema = ?) + \
                     (SELECT COUNT(*) FROM information_schema.routines WHERE routine_schema = ?) AS n"
                        .into(),
                    vec![template.clone().into(), template.clone().into(), template.clone().into()],
                ))
                .await
                .expect("count schema objects")
                .and_then(|row| row.try_get::<i64>("", "n").ok());
            assert_eq!(objects, Some(0), "MySQL test templates copy tables only (no views, triggers or routines)");
            admin
                .execute_unprepared(&format!("CREATE TABLE {template}.{READY} (id int)"))
                .await
                .expect("mark template");
        }
        // A published template never changes: copies need no lock.
        admin.query_one_raw(one("SELECT RELEASE_LOCK('rustcms_template')".into(), vec![])).await.ok();
        admin
            .execute_unprepared(&format!("CREATE DATABASE {name} {MYSQL_DATABASE}"))
            .await
            .expect("create test database");
        let tables: Vec<String> = admin
            .query_all_raw(one(
                "SELECT table_name AS t FROM information_schema.tables WHERE table_schema = ? AND table_type = 'BASE TABLE' AND table_name <> ?"
                    .into(),
                vec![template.clone().into(), READY.into()],
            ))
            .await
            .expect("list template tables")
            .iter()
            .map(|row| row.try_get::<String>("", "t").expect("table name"))
            .collect();
        admin.execute_unprepared(&format!("USE {name}")).await.expect("use test database");
        admin.execute_unprepared("SET FOREIGN_KEY_CHECKS = 0").await.expect("foreign key checks");
        for table in &tables {
            let create = admin
                .query_one_raw(one(format!("SHOW CREATE TABLE {template}.`{table}`"), vec![]))
                .await
                .expect("show create table")
                .expect("table definition")
                .try_get_by_index::<String>(1)
                .expect("create statement");
            admin.execute_unprepared(&create).await.expect("create test table");
            // Writable columns only (generated ones are computed again).
            let columns: Vec<String> = admin
                .query_all_raw(one(
                    "SELECT column_name AS c FROM information_schema.columns WHERE table_schema = ? AND table_name = ? \
                     AND generation_expression = '' ORDER BY ordinal_position"
                        .into(),
                    vec![template.clone().into(), table.clone().into()],
                ))
                .await
                .expect("list template columns")
                .iter()
                .map(|row| format!("`{}`", row.try_get::<String>("", "c").expect("column name")))
                .collect();
            let columns = columns.join(", ");
            admin
                .execute_unprepared(&format!(
                    "INSERT INTO {name}.`{table}` ({columns}) SELECT {columns} FROM {template}.`{table}`"
                ))
                .await
                .expect("copy template rows");
        }
        admin.execute_unprepared("SET FOREIGN_KEY_CHECKS = 1").await.ok();
        admin.close().await.ok();
        let url = replace_database(&admin_url, &name);
        let conn = connect(&url).await;
        Some(Self { conn, url, backend: Backend::Mysql, admin_url: Some(admin_url), name })
    }

    /// Runs every migration of `M` on this database.
    pub async fn migrate<M: MigratorTrait>(&self) {
        M::up(&self.conn, None).await.expect("migrate test database");
    }

    /// Connection pool.
    pub fn conn(&self) -> &DatabaseConnection {
        &self.conn
    }

    /// Connection URL.
    pub fn url(&self) -> &str {
        &self.url
    }

    /// Engine.
    pub fn backend(&self) -> Backend {
        self.backend
    }

    /// Closes the pool and deletes the database.
    pub async fn cleanup(self) {
        self.conn.close().await.ok();
        match (self.backend, self.admin_url) {
            (Backend::Postgres, Some(admin_url)) => {
                if let Ok(admin) = Database::connect(&admin_url).await {
                    admin.execute_unprepared(&format!("DROP DATABASE IF EXISTS {} WITH (FORCE)", self.name)).await.ok();
                    admin.close().await.ok();
                }
            }
            (Backend::Mysql, Some(admin_url)) => {
                if let Ok(admin) = Database::connect(&admin_url).await {
                    admin.execute_unprepared(&format!("DROP DATABASE IF EXISTS {}", self.name)).await.ok();
                    admin.close().await.ok();
                }
            }
            _ => {
                for suffix in ["", "-wal", "-shm"] {
                    std::fs::remove_file(format!("{}{suffix}", self.name)).ok();
                }
            }
        }
    }
}

async fn connect(url: &str) -> DatabaseConnection {
    let config = DatabaseConfig { url: url.to_owned(), max_connections: 5, log_statements: false };
    rustcms_core::db::connect(&config).await.expect("connect to test database")
}

/// One connection, so the advisory lock and the statements share a session.
async fn admin_connection(url: &str) -> DatabaseConnection {
    let mut options = ConnectOptions::new(url);
    options.max_connections(1).min_connections(1).sqlx_logging(false);
    Database::connect(options).await.expect("connect to the test database server")
}

fn replace_database(url: &str, name: &str) -> String {
    format!("{}/{name}", &url[..url.rfind('/').expect("the test database URL must name a database")])
}

/// FNV-1a over the migration names; stable across runs and platforms.
fn fingerprint<'a>(names: impl Iterator<Item = &'a str>) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in names.flat_map(|n| n.bytes().chain([0])) {
        hash = (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3);
    }
    hash
}

/// Creates a table from a statement built for entities
/// (`Schema::create_table_from_entity`), as migrations would on this database:
/// MySQL instants become `DATETIME(6)` (its `TIMESTAMP` stops in 2038 and drops
/// fractions of a second; see `rustcms_migration::schema`).
pub async fn create_table(db: &DatabaseConnection, statement: &sea_orm::sea_query::TableCreateStatement) {
    if db.get_database_backend() != DbBackend::MySql {
        db.execute(statement).await.expect("create table");
        return;
    }
    let sql = statement
        .to_string(sea_orm::sea_query::MysqlQueryBuilder)
        .replace(" timestamp ", " datetime(6) ")
        .replace(" timestamp,", " datetime(6),")
        .replace(" timestamp)", " datetime(6))")
        .replace("DEFAULT CURRENT_TIMESTAMP", "DEFAULT CURRENT_TIMESTAMP(6)")
        .replace(" datetime ", " datetime(6) ")
        .replace(" datetime,", " datetime(6),")
        .replace(" datetime)", " datetime(6))");
    db.execute_unprepared(&sql).await.expect("create table");
}
