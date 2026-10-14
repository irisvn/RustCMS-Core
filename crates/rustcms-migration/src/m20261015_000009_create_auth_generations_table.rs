//! Counters of changes that every instance must see on its next request:
//! `rbac` advances in every RBAC transaction, and permission caches are keyed
//! by it, so no instance serves grants from before a committed change.

use sea_orm_migration::prelude::*;

use crate::schema::create_table;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[sea_orm_migration::async_trait::async_trait]
impl MigrationTrait for Migration {
    fn use_transaction(&self) -> Option<bool> {
        Some(true)
    }

    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        create_table(
            manager,
            Table::create()
                .table(Generations::Table)
                .col(ColumnDef::new(Generations::Name).string_len(32).not_null().primary_key())
                .col(ColumnDef::new(Generations::Value).big_integer().not_null().default(0))
                .to_owned(),
        )
        .await?;
        let existing = Query::select()
            .column(Generations::Name)
            .from(Generations::Table)
            .and_where(Expr::col(Generations::Name).eq("rbac"))
            .to_owned();
        if manager.get_connection().query_one(&existing).await?.is_none() {
            let insert = Query::insert()
                .into_table(Generations::Table)
                .columns([Generations::Name, Generations::Value])
                .values_panic(["rbac".into(), 0.into()])
                .to_owned();
            manager.exec_stmt(insert).await?;
        }
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.drop_table(Table::drop().table(Generations::Table).if_exists().to_owned()).await
    }
}

#[derive(DeriveIden)]
enum Generations {
    #[sea_orm(iden = "auth_generations")]
    Table,
    Name,
    Value,
}
