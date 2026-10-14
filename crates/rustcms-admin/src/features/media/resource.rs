//! The `media` resource: owner scope, soft deletes, its policy, and the
//! queue of objects to delete.

use super::*;

/// The media library resource: listed, viewed, trashed and restored like any
/// resource; created only by uploads and never edited.
pub struct MediaResource;

#[async_trait]
impl Resource for MediaResource {
    type Entity = entity::Entity;
    const SLUG: &'static str = SLUG;

    fn meta() -> ResourceMeta {
        ResourceMeta::new("Media").plural("Media").title("name").icon("image").group("Content").soft_deletes()
    }

    fn fields() -> Vec<Field> {
        let shown = |column: entity::Column| Field::column(column).guarded().readable(|_| true);
        vec![
            shown(entity::Column::Name).searchable().sortable(),
            shown(entity::Column::Mime).filterable(),
            shown(entity::Column::Size).sortable(),
            shown(entity::Column::Width),
            shown(entity::Column::Height),
            shown(entity::Column::OwnerId),
            shown(entity::Column::CreatedAt).sortable(),
            Field::column(entity::Column::DeletedAt).guarded().readable(|_| true),
        ]
    }

    fn form() -> crate::Form {
        crate::Form::new().schema(components![crate::TextInput::make("name")])
    }

    fn table() -> Table {
        Table::new()
            .columns(vec![
                TextColumn::make("name"),
                TextColumn::make("mime"),
                TextColumn::make("size"),
                crate::DateColumn::make("created_at"),
            ])
            .default_sort("-created_at")
    }

    fn policy() -> Arc<dyn Policy> {
        Arc::new(MediaPolicy)
    }

    /// Own uploads only, unless `media.manage_all`.
    fn scope(query: sea_orm::Select<entity::Entity>, ctx: &Ctx) -> sea_orm::Select<entity::Entity> {
        if ctx.can("media.manage_all") {
            return query;
        }
        let owner = ctx.principal.id.parse::<i64>().unwrap_or(-1);
        query.filter(entity::Column::OwnerId.eq(owner))
    }

    async fn after_force_delete(_ctx: &Ctx, txn: &DatabaseTransaction, record: &entity::Model) -> AppResult<()> {
        queue_deletion(txn, &record.disk, &record.key).await
    }
}

/// `media.{ability}`; uploads need `media.create` (checked by the upload
/// route); the generic create and update endpoints are always refused.
pub struct MediaPolicy;

#[async_trait]
impl Policy for MediaPolicy {
    async fn allows(&self, ctx: &Ctx, ability: &str, _record: Option<&Data>) -> bool {
        !matches!(ability, "create" | "update") && ctx.can(&format!("{SLUG}.{ability}"))
    }

    async fn admits(&self, ctx: &Ctx, ability: &str) -> bool {
        self.allows(ctx, ability, None).await
    }

    fn view_depends_on_record(&self) -> bool {
        false
    }
}

pub(super) async fn queue_deletion(txn: &DatabaseTransaction, disk: &str, key: &str) -> AppResult<()> {
    deletions::ActiveModel {
        id: Set(Uuid::now_v7()),
        disk: Set(disk.to_owned()),
        key: Set(key.to_owned()),
        attempts: Set(0),
        next_at: Set(Utc::now()),
    }
    .insert(txn)
    .await?;
    Ok(())
}
