//! The `media` resource: owner scope, soft deletes, its policy, and the
//! queue of objects to delete.

use super::*;

/// The media library resource: listed, viewed, trashed and restored like any
/// resource; created only by uploads. Only its `name` (a rename) and `alt`
/// are edited, by the uploader or with `media.manage_all`.
pub struct MediaResource;

/// Longest alternative text, in characters.
pub const MAX_ALT: usize = 500;

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
            // Renamed on the record (cleaned like an uploaded name).
            Field::column(entity::Column::Name)
                .readable(|_| true)
                .creatable(|_| false)
                .required()
                .max_length(MAX_NAME)
                .searchable()
                .sortable(),
            Field::column(entity::Column::Alt)
                .label("Alternative text")
                .readable(|_| true)
                .creatable(|_| false)
                .max_length(MAX_ALT),
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
        crate::Form::new().schema(components![crate::TextInput::make("name"), crate::Textarea::make("alt")])
    }

    /// A new name is cleaned as uploaded names are (no path, quotes or
    /// control and direction characters).
    async fn mutate_before_update(
        _ctx: &Ctx,
        _txn: &DatabaseTransaction,
        _record: &entity::Model,
        mut data: Data,
    ) -> AppResult<Data> {
        if let Some(serde_json::Value::String(name)) = data.get("name") {
            // Nothing left of it: refused (uploads fall back to `file`).
            let cleaned = cleaned_name(name).ok_or_else(|| ValidationErrors::field("name", "is required"))?;
            data.insert("name".into(), serde_json::Value::String(cleaned));
        }
        Ok(data)
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

    /// Own uploads only, unless `media.view_all` (see and reference every
    /// file) or `media.manage_all` (also trash, restore and delete them).
    fn scope(query: sea_orm::Select<entity::Entity>, ctx: &Ctx) -> sea_orm::Select<entity::Entity> {
        if ctx.can(MANAGE_ALL) || ctx.can(VIEW_ALL) {
            return query;
        }
        let owner = ctx.principal.id.parse::<i64>().unwrap_or(-1);
        query.filter(entity::Column::OwnerId.eq(owner))
    }

    async fn after_force_delete(_ctx: &Ctx, txn: &DatabaseTransaction, record: &entity::Model) -> AppResult<()> {
        queue_deletion(txn, &record.disk, &record.key).await
    }
}

/// Uploads files (`POST {admin}/api/media`); the media schema says so as the
/// `upload` ability.
pub(crate) const UPLOAD: &str = "media.create";

/// Sees every file and may reference it, without trashing, restoring or
/// deleting other users' files.
pub const VIEW_ALL: &str = "media.view_all";

/// Sees every file and may trash, restore and delete it (with `media.delete`,
/// `media.restore`, `media.force_delete`).
pub const MANAGE_ALL: &str = "media.manage_all";

/// How the Roles page shows the library's own permissions, unless the
/// application declares `permission_meta` for them (`manage_all` includes
/// everything `view_all` grants).
pub(crate) fn permission_meta(name: &str) -> Option<rustcms_auth::PermissionMeta> {
    let meta = rustcms_auth::PermissionMeta::new().area(rustcms_auth::catalog::RESOURCES_AREA);
    match name {
        VIEW_ALL => Some(meta.label("See every file").description(
            "List, open and use every file of the media library in records, not only one's own uploads. \
             Editing, trashing, restoring or deleting another user's file needs \"Manage every file\".",
        )),
        MANAGE_ALL => Some(meta.label("Manage every file").description(
            "See every file, and edit, trash, restore or delete other users' files (with the update, delete, \
             restore and force delete permissions of the media library).",
        )),
        _ => None,
    }
}

/// `media.{ability}`; uploads need `media.create` (checked by the upload
/// route); the generic create endpoint is always refused. Updating (renaming,
/// alternative text), trashing, restoring and deleting another user's file
/// also needs `media.manage_all` (`media.view_all` only shows it).
pub struct MediaPolicy;

#[async_trait]
impl Policy for MediaPolicy {
    async fn allows(&self, ctx: &Ctx, ability: &str, record: Option<&Data>) -> bool {
        if ability == "create" || !ctx.can(&format!("{SLUG}.{ability}")) {
            return false;
        }
        match (ability, record) {
            ("update" | "delete" | "restore" | "force_delete", Some(record)) if !ctx.can(MANAGE_ALL) => {
                // `owner_id` is an i64: a string on the wire (ADR 0003).
                record.get("owner_id").and_then(|v| v.as_str()) == Some(ctx.principal.id.as_str())
            }
            _ => true,
        }
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
