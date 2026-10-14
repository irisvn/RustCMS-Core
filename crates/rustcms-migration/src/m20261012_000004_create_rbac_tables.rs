//! Roles and permissions (spatie/laravel-permission style), scoped by guard.
//! Seeds the system `super_admin` role of each framework guard.

use sea_orm_migration::prelude::*;

use crate::schema::{ColumnDefExt, create_index, create_table};

#[derive(DeriveMigrationName)]
pub struct Migration;

const GUARDS: [&str; 2] = ["admin", "api"];

#[sea_orm_migration::async_trait::async_trait]
impl MigrationTrait for Migration {
    fn use_transaction(&self) -> Option<bool> {
        Some(true)
    }

    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let created_at = || ColumnDef::new(Alias::new("created_at")).instant_now(manager).not_null().to_owned();
        create_table(
            manager,
            Table::create()
                .table(Permissions::Table)
                .col(ColumnDef::new(Permissions::Id).big_integer().not_null().auto_increment().primary_key())
                .col(ColumnDef::new(Permissions::Name).string_len(255).not_null())
                .col(ColumnDef::new(Permissions::GuardName).string_len(64).not_null())
                .col(ColumnDef::new(Permissions::Group).string_len(255).null())
                .col(ColumnDef::new(Permissions::Orphaned).boolean().not_null().default(false))
                .col(created_at())
                .index(
                    Index::create()
                        .name("permissions_name_guard_key")
                        .col(Permissions::Name)
                        .col(Permissions::GuardName)
                        .unique(),
                )
                .to_owned(),
        )
        .await?;
        create_table(
            manager,
            Table::create()
                .table(Roles::Table)
                .col(ColumnDef::new(Roles::Id).big_integer().not_null().auto_increment().primary_key())
                .col(ColumnDef::new(Roles::Name).string_len(255).not_null())
                .col(ColumnDef::new(Roles::GuardName).string_len(64).not_null())
                .col(ColumnDef::new(Roles::IsSystem).boolean().not_null().default(false))
                .col(ColumnDef::new(Roles::IsPrivileged).boolean().not_null().default(false))
                .col(created_at())
                .index(Index::create().name("roles_name_guard_key").col(Roles::Name).col(Roles::GuardName).unique())
                .to_owned(),
        )
        .await?;
        create_table(
            manager,
            Table::create()
                .table(RoleHasPermissions::Table)
                .col(ColumnDef::new(RoleHasPermissions::RoleId).big_integer().not_null())
                .col(ColumnDef::new(RoleHasPermissions::PermissionId).big_integer().not_null())
                .primary_key(Index::create().col(RoleHasPermissions::RoleId).col(RoleHasPermissions::PermissionId))
                .foreign_key(
                    ForeignKey::create()
                        .from(RoleHasPermissions::Table, RoleHasPermissions::RoleId)
                        .to(Roles::Table, Roles::Id)
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .foreign_key(
                    ForeignKey::create()
                        .from(RoleHasPermissions::Table, RoleHasPermissions::PermissionId)
                        .to(Permissions::Table, Permissions::Id)
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .to_owned(),
        )
        .await?;
        create_table(
            manager,
            Table::create()
                .table(ModelHasRoles::Table)
                .col(ColumnDef::new(ModelHasRoles::RoleId).big_integer().not_null())
                .col(ColumnDef::new(ModelHasRoles::ModelType).string_len(255).not_null())
                .col(ColumnDef::new(ModelHasRoles::ModelId).string_len(255).not_null())
                .primary_key(
                    Index::create()
                        .col(ModelHasRoles::RoleId)
                        .col(ModelHasRoles::ModelType)
                        .col(ModelHasRoles::ModelId),
                )
                .foreign_key(
                    ForeignKey::create()
                        .from(ModelHasRoles::Table, ModelHasRoles::RoleId)
                        .to(Roles::Table, Roles::Id)
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .to_owned(),
        )
        .await?;
        create_index(
            manager,
            ModelHasRoles::Table,
            Index::create()
                .name("model_has_roles_model_idx")
                .table(ModelHasRoles::Table)
                .col(ModelHasRoles::ModelType)
                .col(ModelHasRoles::ModelId)
                .to_owned(),
        )
        .await?;
        create_table(
            manager,
            Table::create()
                .table(ModelHasPermissions::Table)
                .col(ColumnDef::new(ModelHasPermissions::PermissionId).big_integer().not_null())
                .col(ColumnDef::new(ModelHasPermissions::ModelType).string_len(255).not_null())
                .col(ColumnDef::new(ModelHasPermissions::ModelId).string_len(255).not_null())
                .primary_key(
                    Index::create()
                        .col(ModelHasPermissions::PermissionId)
                        .col(ModelHasPermissions::ModelType)
                        .col(ModelHasPermissions::ModelId),
                )
                .foreign_key(
                    ForeignKey::create()
                        .from(ModelHasPermissions::Table, ModelHasPermissions::PermissionId)
                        .to(Permissions::Table, Permissions::Id)
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .to_owned(),
        )
        .await?;
        create_index(
            manager,
            ModelHasPermissions::Table,
            Index::create()
                .name("model_has_permissions_model_idx")
                .table(ModelHasPermissions::Table)
                .col(ModelHasPermissions::ModelType)
                .col(ModelHasPermissions::ModelId)
                .to_owned(),
        )
        .await?;

        // Seeds once: a re-run (MySQL commits DDL, so a failed run is retried on a
        // partly migrated schema) finds them and skips them.
        for guard in GUARDS {
            let existing = Query::select()
                .column(Roles::Id)
                .from(Roles::Table)
                .and_where(Expr::col(Roles::Name).eq("super_admin"))
                .and_where(Expr::col(Roles::GuardName).eq(guard))
                .to_owned();
            if manager.get_connection().query_one(&existing).await?.is_some() {
                continue;
            }
            let insert = Query::insert()
                .into_table(Roles::Table)
                .columns([Roles::Name, Roles::GuardName, Roles::IsSystem, Roles::IsPrivileged])
                .values_panic(["super_admin".into(), guard.into(), true.into(), true.into()])
                .to_owned();
            manager.exec_stmt(insert).await?;
        }
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        for table in [
            ModelHasPermissions::Table.into_iden(),
            ModelHasRoles::Table.into_iden(),
            RoleHasPermissions::Table.into_iden(),
            Roles::Table.into_iden(),
            Permissions::Table.into_iden(),
        ] {
            manager.drop_table(Table::drop().table(table).if_exists().to_owned()).await?;
        }
        Ok(())
    }
}

#[derive(DeriveIden)]
enum Permissions {
    Table,
    Id,
    Name,
    GuardName,
    Group,
    Orphaned,
}

#[derive(DeriveIden)]
enum Roles {
    Table,
    Id,
    Name,
    GuardName,
    IsSystem,
    IsPrivileged,
}

#[derive(DeriveIden)]
enum RoleHasPermissions {
    Table,
    RoleId,
    PermissionId,
}

#[derive(DeriveIden)]
enum ModelHasRoles {
    Table,
    RoleId,
    ModelType,
    ModelId,
}

#[derive(DeriveIden)]
enum ModelHasPermissions {
    Table,
    PermissionId,
    ModelType,
    ModelId,
}
