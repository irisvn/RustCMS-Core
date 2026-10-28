//! `integrations` table: its secrets are encrypted columns.

use std::collections::BTreeMap;

use rustcms::crypto::Encrypted;
use sea_orm::entity::prelude::*;

// #region model
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "integrations")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub name: String,
    #[sea_orm(column_type = "Text", nullable)]
    pub api_token: Option<Encrypted<String>>,
    #[sea_orm(column_type = "Text", nullable)]
    pub env: Option<Encrypted<BTreeMap<String, String>>>,
}
// #endregion model

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
