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
}

/// An isolated, initially empty database owned by one test.
///
/// - SQLite: a temp file, opened with the same options as production
///   (foreign keys, WAL, busy timeout), so locking behaves like the real thing.
/// - PostgreSQL: a new database created on the server in `RUSTCMS_TEST_PG`.
///   No `search_path` tricks: every pooled connection sees the same database.
#[derive(Clone)]
pub struct TestDb {
    conn: DatabaseConnection,
    url: String,
    backend: Backend,
    pg_admin_url: Option<String>,
    name: String,
}

const ENV: &str = "RUSTCMS_TEST_PG";
/// Advisory lock serializing template creation across test processes.
const LOCK: i64 = 0x7275_7374_636d_7301;

impl TestDb {
    /// A fresh SQLite database file.
    pub async fn sqlite() -> Self {
        let path = std::env::temp_dir().join(format!("rustcms-test-{}.db", Uuid::new_v4()));
        let url = format!("sqlite://{}?mode=rwc", path.display());
        let conn = connect(&url).await;
        Self { conn, url, backend: Backend::Sqlite, pg_admin_url: None, name: path.display().to_string() }
    }

    /// A fresh empty database, or `None` for PostgreSQL when `RUSTCMS_TEST_PG` is not set.
    pub async fn empty(backend: Backend) -> Option<Self> {
        match backend {
            Backend::Sqlite => Some(Self::sqlite().await),
            Backend::Postgres => Self::postgres().await,
        }
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
        Some(Self { conn, url, backend: Backend::Postgres, pg_admin_url: Some(admin_url), name })
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
                Some(Self { conn, url, backend, pg_admin_url: Some(admin_url), name })
            }
        }
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
        match (self.backend, self.pg_admin_url) {
            (Backend::Postgres, Some(admin_url)) => {
                if let Ok(admin) = Database::connect(&admin_url).await {
                    admin.execute_unprepared(&format!("DROP DATABASE IF EXISTS {} WITH (FORCE)", self.name)).await.ok();
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
    Database::connect(options).await.expect("connect to RUSTCMS_TEST_PG")
}

fn replace_database(url: &str, name: &str) -> String {
    format!("{}/{name}", &url[..url.rfind('/').expect("RUSTCMS_TEST_PG must name a database")])
}

/// FNV-1a over the migration names; stable across runs and platforms.
fn fingerprint<'a>(names: impl Iterator<Item = &'a str>) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in names.flat_map(|n| n.bytes().chain([0])) {
        hash = (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3);
    }
    hash
}
