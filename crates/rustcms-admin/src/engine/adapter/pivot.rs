//! Pivot tables of belongs-to-many relations: generic over the pivot entity,
//! erased to [`DynPivot`]. Rows are addressed by their two key columns (the
//! parent's and the target's), never by the pivot's own primary key.

use super::*;
use sea_orm::sea_query::{Query, SelectStatement};

/// A pivot column: its kind and whether inserts may leave it out.
#[derive(Debug, Clone, Copy)]
pub(crate) struct PivotColumn {
    pub(crate) kind: FieldKind,
    pub(crate) nullable: bool,
    pub(crate) has_default: bool,
}

/// Object-safe view of a pivot table.
#[async_trait]
pub(crate) trait DynPivot: Send + Sync {
    /// Table name.
    fn table(&self) -> &str;
    /// Column holding the parent's key.
    fn parent_column(&self) -> &str;
    /// Column holding the target's key.
    fn target_column(&self) -> &str;
    /// Every supported column, by name.
    fn column(&self, name: &str) -> Option<PivotColumn>;
    /// Names of every column.
    fn column_names(&self) -> Vec<String>;
    /// `SELECT target FROM pivot WHERE parent = $parent` (lists of one parent's targets).
    fn targets_of(&self, parent: Value) -> SelectStatement;
    /// The rows of `parent` among `targets`: target key → every column in wire format.
    async fn rows(&self, conn: DbConn<'_>, parent: Value, targets: Vec<Value>) -> AppResult<BTreeMap<String, Data>>;
    /// Inserts the row `(parent, target)` with `values` (validated pivot columns).
    async fn insert(&self, txn: &DatabaseTransaction, parent: Value, target: Value, values: &Data) -> AppResult<()>;
    /// Deletes the rows of `parent` to `targets`; returns how many.
    async fn delete(&self, txn: &DatabaseTransaction, parent: Value, targets: Vec<Value>) -> AppResult<u64>;
    /// Writes `values` into the row `(parent, target)`; `false` when there is none.
    async fn update(&self, txn: &DatabaseTransaction, parent: Value, target: Value, values: &Data) -> AppResult<bool>;
}

/// A connection or a transaction, for reads that run in either.
pub(crate) enum DbConn<'a> {
    /// Outside any transaction.
    Pool(&'a sea_orm::DatabaseConnection),
    /// In a write transaction.
    Txn(&'a DatabaseTransaction),
}

/// [`DynPivot`] for the entity `P`.
pub(crate) struct PivotAdapter<P: EntityTrait> {
    table: String,
    parent: (P::Column, String),
    target: (P::Column, String),
    columns: HashMap<String, (P::Column, PivotColumn)>,
}

impl<P: EntityTrait> PivotAdapter<P> {
    /// # Panics
    /// When `parent` or `target` is not a supported column of `P`, or both are the same.
    pub(crate) fn new(relation: &str, parent: &str, target: &str) -> Self {
        let columns: HashMap<String, (P::Column, PivotColumn)> = P::Column::iter()
            .filter_map(|column| {
                let def = column.def();
                let kind = FieldKind::from_column_type(def.get_column_type())?;
                let info =
                    PivotColumn { kind, nullable: def.is_null(), has_default: def.get_column_default().is_some() };
                Some((column.as_str().to_owned(), (column, info)))
            })
            .collect();
        let table = P::default().table_name().to_owned();
        let find = |name: &str| {
            let (column, _) = columns
                .get(name)
                .unwrap_or_else(|| panic!("relation `{relation}`: pivot `{table}` has no supported column `{name}`"));
            (*column, name.to_owned())
        };
        assert!(parent != target, "relation `{relation}`: the pivot's two key columns must differ");
        Self { parent: find(parent), target: find(target), table, columns }
    }

    fn set(&self, model: &mut P::ActiveModel, name: &str, value: &serde_json::Value) -> AppResult<()> {
        let (column, info) = self
            .columns
            .get(name)
            .ok_or_else(|| AppError::internal(format!("pivot `{}`: no column `{name}`", self.table)))?;
        let candidates = info.kind.decode(value).map_err(|e| AppError::internal(e.0))?;
        if candidates.into_iter().any(|v| model.try_set(*column, v).is_ok()) {
            Ok(())
        } else {
            Err(AppError::internal(format!("pivot `{}`: column `{name}` does not match its wire kind", self.table)))
        }
    }

    fn of(&self, parent: Value, targets: Vec<Value>) -> sea_orm::Select<P> {
        P::find().filter(self.parent.0.eq(parent)).filter(self.target.0.is_in(targets))
    }
}

#[async_trait]
impl<P: EntityTrait> DynPivot for PivotAdapter<P>
where
    P::Model: IntoActiveModel<P::ActiveModel> + Sync,
    P::ActiveModel: Send,
{
    fn table(&self) -> &str {
        &self.table
    }

    fn parent_column(&self) -> &str {
        &self.parent.1
    }

    fn target_column(&self) -> &str {
        &self.target.1
    }

    fn column(&self, name: &str) -> Option<PivotColumn> {
        self.columns.get(name).map(|(_, info)| *info)
    }

    fn column_names(&self) -> Vec<String> {
        self.columns.keys().cloned().collect()
    }

    fn targets_of(&self, parent: Value) -> SelectStatement {
        Query::select()
            .column((P::default(), self.target.0))
            .from(P::default())
            .and_where(self.parent.0.eq(parent))
            .to_owned()
    }

    async fn rows(&self, conn: DbConn<'_>, parent: Value, targets: Vec<Value>) -> AppResult<BTreeMap<String, Data>> {
        if targets.is_empty() {
            return Ok(BTreeMap::new());
        }
        let select = self.of(parent, targets);
        let models = match conn {
            DbConn::Pool(db) => select.all(db).await?,
            DbConn::Txn(txn) => select.all(txn).await?,
        };
        Ok(models
            .iter()
            .map(|model| {
                let data: Data = P::Column::iter()
                    .map(|c| (c.as_str().to_owned(), encode_column(c.def().get_column_type(), &model.get(c))))
                    .collect();
                (encode_key(&model.get(self.target.0)), data)
            })
            .collect())
    }

    async fn insert(&self, txn: &DatabaseTransaction, parent: Value, target: Value, values: &Data) -> AppResult<()> {
        let mut model = <P::ActiveModel as ActiveModelTrait>::default();
        model.try_set(self.parent.0, parent).map_err(AppError::internal)?;
        model.try_set(self.target.0, target).map_err(AppError::internal)?;
        for (name, value) in values {
            self.set(&mut model, name, value)?;
        }
        P::insert(model).exec_without_returning(txn).await.map_err(link_err)?;
        Ok(())
    }

    async fn delete(&self, txn: &DatabaseTransaction, parent: Value, targets: Vec<Value>) -> AppResult<u64> {
        if targets.is_empty() {
            return Ok(0);
        }
        let result =
            P::delete_many().filter(self.parent.0.eq(parent)).filter(self.target.0.is_in(targets)).exec(txn).await?;
        Ok(result.rows_affected)
    }

    async fn update(&self, txn: &DatabaseTransaction, parent: Value, target: Value, values: &Data) -> AppResult<bool> {
        let Some(row) = self.of(parent, vec![target]).one(txn).await? else { return Ok(false) };
        if values.is_empty() {
            return Ok(true);
        }
        let mut model = row.into_active_model();
        for (name, value) in values {
            self.set(&mut model, name, value)?;
        }
        model.update(txn).await.map_err(link_err)?;
        Ok(true)
    }
}

/// A pivot write failing on a foreign key (a target or parent gone, or
/// protected): a form error, like a resource write.
fn link_err(err: DbErr) -> AppError {
    if matches!(err.sql_err(), Some(SqlErr::ForeignKeyConstraintViolation(_))) {
        let mut errors = ValidationErrors::new();
        errors.add_form("references a missing or protected record");
        return errors.into();
    }
    err.into()
}
