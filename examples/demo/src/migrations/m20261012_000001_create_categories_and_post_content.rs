use rustcms::migration::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    fn use_transaction(&self) -> Option<bool> {
        Some(true)
    }

    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(Categories::Table)
                    .col(ColumnDef::new(Categories::Id).integer().not_null().auto_increment().primary_key())
                    .col(ColumnDef::new(Categories::Name).string_len(120).not_null())
                    .col(ColumnDef::new(Categories::Slug).string_len(120).not_null())
                    .index(Index::create().name("categories_slug_key").col(Categories::Slug).unique())
                    .to_owned(),
            )
            .await?;
        // Rebuild `posts` with the new columns and a real foreign key (SQLite cannot
        // add constraints to an existing table): deleting a category clears it on posts.
        // Ids never go backwards: the highest id ever issued carries over.
        lock_posts(manager).await?;
        let high_water = id_high_water(manager).await?;
        manager
            .create_table(
                Table::create()
                    .table(PostsNew::Table)
                    .col(ColumnDef::new(Posts::Id).big_integer().not_null().auto_increment().primary_key())
                    .col(ColumnDef::new(Posts::Title).string_len(200).not_null().default(""))
                    .col(ColumnDef::new(Posts::Body).text().null())
                    .col(ColumnDef::new(Posts::CategoryId).integer().null())
                    .col(ColumnDef::new(Posts::AuthorId).big_integer().null())
                    .col(ColumnDef::new(Posts::Published).boolean().not_null().default(false))
                    .col(ColumnDef::new(Posts::PublishedAt).timestamp_with_time_zone().null())
                    .col(
                        ColumnDef::new(Posts::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .col(
                        ColumnDef::new(Posts::UpdatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("posts_category_id_fkey")
                            .from(PostsNew::Table, Posts::CategoryId)
                            .to(Categories::Table, Categories::Id)
                            .on_delete(ForeignKeyAction::SetNull),
                    )
                    .to_owned(),
            )
            .await?;
        let copy = Query::insert()
            .into_table(PostsNew::Table)
            .columns([Posts::Id, Posts::CreatedAt, Posts::UpdatedAt])
            .select_from(
                Query::select().columns([Posts::Id, Posts::CreatedAt, Posts::UpdatedAt]).from(Posts::Table).to_owned(),
            )
            .map_err(|e| DbErr::Custom(e.to_string()))?
            .to_owned();
        manager.exec_stmt(copy).await?;
        manager.drop_table(Table::drop().table(Posts::Table).to_owned()).await?;
        manager.rename_table(Table::rename().table(PostsNew::Table, Posts::Table).to_owned()).await?;
        restore_id_high_water(manager, high_water).await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Posts keep their ids and timestamps; the content columns are dropped.
        lock_posts(manager).await?;
        let high_water = id_high_water(manager).await?;
        manager
            .create_table(
                Table::create()
                    .table(PostsNew::Table)
                    .col(ColumnDef::new(Posts::Id).big_integer().not_null().auto_increment().primary_key())
                    .col(
                        ColumnDef::new(Posts::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .col(
                        ColumnDef::new(Posts::UpdatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .to_owned(),
            )
            .await?;
        let copy = Query::insert()
            .into_table(PostsNew::Table)
            .columns([Posts::Id, Posts::CreatedAt, Posts::UpdatedAt])
            .select_from(
                Query::select().columns([Posts::Id, Posts::CreatedAt, Posts::UpdatedAt]).from(Posts::Table).to_owned(),
            )
            .map_err(|e| DbErr::Custom(e.to_string()))?
            .to_owned();
        manager.exec_stmt(copy).await?;
        manager.drop_table(Table::drop().table(Posts::Table).to_owned()).await?;
        manager.rename_table(Table::rename().table(PostsNew::Table, Posts::Table).to_owned()).await?;
        restore_id_high_water(manager, high_water).await?;
        manager.drop_table(Table::drop().table(Categories::Table).to_owned()).await
    }
}

/// Keeps writers out of `posts` until the migration commits, so no write lands
/// between the copy and the swap (SQLite: the migration already holds the write lock).
async fn lock_posts(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    if manager.get_database_backend() == sea_orm::DbBackend::Postgres {
        manager.get_connection().execute_unprepared("LOCK TABLE posts IN ACCESS EXCLUSIVE MODE").await?;
    }
    Ok(())
}

/// The highest `posts.id` ever issued (sequence state, or the largest id).
async fn id_high_water(manager: &SchemaManager<'_>) -> Result<i64, DbErr> {
    let sql = match manager.get_database_backend() {
        sea_orm::DbBackend::Postgres => {
            "SELECT GREATEST(COALESCE((SELECT last_value FROM pg_sequences \
             WHERE schemaname || '.' || sequencename = pg_get_serial_sequence('posts', 'id')), 0), \
             COALESCE((SELECT MAX(id) FROM posts), 0))::bigint AS n"
        }
        _ => {
            "SELECT MAX(COALESCE((SELECT seq FROM sqlite_sequence WHERE name = 'posts'), 0), \
             COALESCE((SELECT MAX(id) FROM posts), 0)) AS n"
        }
    };
    let row = manager
        .get_connection()
        .query_one_raw(sea_orm::Statement::from_string(manager.get_database_backend(), sql))
        .await?;
    Ok(row.map(|r| r.try_get::<i64>("", "n")).transpose()?.unwrap_or(0))
}

/// Makes the rebuilt `posts` table continue after `high_water`.
async fn restore_id_high_water(manager: &SchemaManager<'_>, high_water: i64) -> Result<(), DbErr> {
    let db = manager.get_connection();
    match manager.get_database_backend() {
        sea_orm::DbBackend::Postgres => {
            let sql = if high_water > 0 {
                format!("SELECT setval(pg_get_serial_sequence('posts', 'id'), {high_water}, true)")
            } else {
                "SELECT setval(pg_get_serial_sequence('posts', 'id'), 1, false)".to_owned()
            };
            db.execute_unprepared(&sql).await?;
        }
        _ => {
            db.execute_unprepared("DELETE FROM sqlite_sequence WHERE name = 'posts'").await?;
            db.execute_unprepared(&format!("INSERT INTO sqlite_sequence (name, seq) VALUES ('posts', {high_water})"))
                .await?;
        }
    }
    Ok(())
}

#[derive(DeriveIden)]
enum Categories {
    Table,
    Id,
    Name,
    Slug,
}

#[derive(DeriveIden)]
enum PostsNew {
    Table,
}

#[derive(DeriveIden)]
enum Posts {
    Table,
    Id,
    CreatedAt,
    UpdatedAt,
    Title,
    Body,
    CategoryId,
    AuthorId,
    Published,
    PublishedAt,
}
