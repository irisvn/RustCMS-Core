//! Registered encrypted columns and `key:reencrypt` (ADR 0030).

use std::collections::BTreeMap;

use async_trait::async_trait;
use clap::{Arg, ArgAction, ArgMatches, Command};
use sea_orm::sea_query::{Alias, Expr, ExprTrait, IntoTableRef, Order, Query, TableRef};
use sea_orm::{
    ColumnTrait, ColumnType, ConnectionTrait, DbBackend, EntityTrait, IdenStatic, Iterable, PrimaryKeyToColumn,
    QueryResult, Value,
};

use super::{Context, Encrypted, Keyring, RowIdentity};
use crate::{AppCommand, AppError, AppResult, AppState};

/// What a column's ciphertexts are bound to (see [`Context`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Binding {
    /// Its table and column.
    Column,
    /// Its table, column and row (its primary key, known before the insert).
    Row,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum KeyKind {
    Integer,
    Text,
    Uuid,
}

/// An encrypted column, as registered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncryptedColumn {
    schema: Option<String>,
    table: String,
    column: String,
    key: String,
    key_kind: KeyKind,
    binding: Binding,
}

impl EncryptedColumn {
    /// Column `column` of entity `E`.
    ///
    /// # Panics
    /// On a composite primary key, or one that is neither an integer, text
    /// nor a UUID.
    pub fn of<E: EntityTrait>(column: E::Column, binding: Binding) -> Self {
        let entity = E::default();
        let keys: Vec<E::Column> = E::PrimaryKey::iter().map(PrimaryKeyToColumn::into_column).collect();
        let table = entity.table_name().to_owned();
        let [key] = keys[..] else {
            panic!("encrypted column {table}.{}: the primary key must be one column", column.as_str())
        };
        let key_kind = match key.def().get_column_type() {
            ColumnType::TinyInteger
            | ColumnType::SmallInteger
            | ColumnType::Integer
            | ColumnType::BigInteger
            | ColumnType::TinyUnsigned
            | ColumnType::SmallUnsigned
            | ColumnType::Unsigned
            | ColumnType::BigUnsigned => KeyKind::Integer,
            ColumnType::Uuid => KeyKind::Uuid,
            ColumnType::Char(_) | ColumnType::String(_) | ColumnType::Text => KeyKind::Text,
            other => panic!("encrypted column {table}.{}: unsupported primary key type {other:?}", column.as_str()),
        };
        Self {
            schema: entity.schema_name().map(str::to_owned),
            table,
            column: column.as_str().to_owned(),
            key: key.as_str().to_owned(),
            key_kind,
            binding,
        }
    }

    /// `schema.table` or `table`.
    pub fn table(&self) -> String {
        match &self.schema {
            Some(schema) => format!("{schema}.{}", self.table),
            None => self.table.clone(),
        }
    }

    /// The column name.
    pub fn column(&self) -> &str {
        &self.column
    }

    /// The context of the row whose key is `row`.
    fn context(&self, row: &str) -> Context {
        let context = Context::column(self.table(), self.column.clone());
        match self.binding {
            Binding::Column => context,
            Binding::Row => context.row(row),
        }
    }

    fn table_ref(&self) -> TableRef {
        match &self.schema {
            Some(schema) => (Alias::new(schema), Alias::new(&self.table)).into_table_ref(),
            None => Alias::new(&self.table).into_table_ref(),
        }
    }

    /// The primary key of `row` as a value, and in canonical text.
    fn key_of(&self, row: &QueryResult) -> AppResult<(Value, String)> {
        Ok(match self.key_kind {
            KeyKind::Integer => {
                let key: i64 = row.try_get_by_index(0)?;
                (Value::from(key), key.canonical())
            }
            KeyKind::Uuid => {
                let key: uuid::Uuid = row.try_get_by_index(0)?;
                (Value::from(key), key.canonical())
            }
            KeyKind::Text => {
                let key: String = row.try_get_by_index(0)?;
                (Value::from(key.clone()), key)
            }
        })
    }
}

/// The columns registered with [`AppBuilder::encrypted_column`](crate::AppBuilder::encrypted_column).
#[derive(Debug, Clone, Default)]
pub struct EncryptedColumns(pub(crate) Vec<EncryptedColumn>);

impl EncryptedColumns {
    /// Adds `column`; the same column twice is kept once.
    ///
    /// # Panics
    /// When the same column is registered with another binding.
    pub(crate) fn add(&mut self, column: EncryptedColumn) {
        match self.0.iter().find(|c| c.table() == column.table() && c.column == column.column) {
            Some(same) if *same == column => {}
            Some(_) => panic!("encrypted column {}.{} is registered twice, differently", column.table(), column.column),
            None => self.0.push(column),
        }
    }

    /// Every registered column.
    pub fn all(&self) -> &[EncryptedColumn] {
        &self.0
    }
}

/// What `key:reencrypt` found in one column (or the settings).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ColumnReport {
    /// Already under the current key (authenticated).
    pub current: usize,
    /// Moved to the current key (with `dry_run`: would be).
    pub moved: usize,
    /// Changed by someone else meanwhile: left for the next run.
    pub changed: usize,
    /// No key opens them: listed by primary key, never modified.
    pub unreadable: Vec<String>,
}

impl ColumnReport {
    fn finished(&self, dry_run: bool) -> bool {
        self.changed == 0 && self.unreadable.is_empty() && !(dry_run && self.moved > 0)
    }
}

/// Moves the ciphertexts of `column` to the current key, `batch` rows at a
/// time in primary-key order (resumes anywhere: done rows are skipped).
pub async fn reencrypt_column<C: ConnectionTrait>(
    db: &C,
    keyring: &Keyring,
    column: &EncryptedColumn,
    batch: u64,
    dry_run: bool,
) -> AppResult<ColumnReport> {
    let mut report = ColumnReport::default();
    let mut after: Option<Value> = None;
    let key = || Expr::col(Alias::new(&column.key));
    loop {
        let mut select = Query::select();
        select
            .column(Alias::new(&column.key))
            .column(Alias::new(&column.column))
            .from(column.table_ref())
            .and_where(Expr::col(Alias::new(&column.column)).is_not_null())
            .order_by(Alias::new(&column.key), Order::Asc)
            .limit(batch);
        if let Some(after) = &after {
            select.and_where(key().gt(after.clone()));
        }
        let rows = db.query_all(&select).await?;
        let count = rows.len();
        for row in &rows {
            let (value, canonical) = column.key_of(row)?;
            let envelope: String = row.try_get_by_index(1)?;
            after = Some(value.clone());
            let context = column.context(&canonical);
            let sealed = Encrypted::<()>::from_envelope(envelope.clone());
            if sealed.is_current(keyring, &context) {
                report.current += 1;
                continue;
            }
            let Ok(fresh) = sealed.resealed(keyring, &context) else {
                report.unreadable.push(canonical);
                continue;
            };
            if dry_run {
                report.moved += 1;
                continue;
            }
            // Compare-and-swap, bytewise (a MySQL collation could make two
            // envelopes equal otherwise).
            let unchanged: Expr = match db.get_database_backend() {
                DbBackend::MySql => {
                    Expr::cust_with_exprs("BINARY ? = ?", [Expr::col(Alias::new(&column.column)), Expr::val(envelope)])
                }
                _ => Expr::col(Alias::new(&column.column)).eq(envelope),
            };
            let mut update = Query::update();
            update
                .table(column.table_ref())
                .value(Alias::new(&column.column), fresh.envelope().to_owned())
                .and_where(key().eq(value))
                .and_where(unchanged);
            let written = db.execute(&update).await?;
            if written.rows_affected() == 1 {
                report.moved += 1;
            } else {
                report.changed += 1;
            }
        }
        if (count as u64) < batch {
            break;
        }
    }
    Ok(report)
}

/// Moves every settings secret to the current key (each group in its own
/// write transaction, under its row lock).
pub async fn reencrypt_settings(state: &AppState, dry_run: bool) -> AppResult<ColumnReport> {
    let mut report = ColumnReport::default();
    for group in crate::settings::groups(state.db()).await? {
        let txn = crate::db::begin_write(state.db()).await?;
        let resealed = crate::settings::reseal(&txn, state.keyring(), &group).await?;
        if dry_run {
            txn.rollback().await?;
        } else {
            txn.commit().await?;
        }
        report.current += resealed.current;
        report.moved += resealed.moved;
        report.unreadable.extend((0..resealed.unreadable).map(|_| group.clone()));
    }
    Ok(report)
}

/// `key:reencrypt`: moves every registered encrypted column and the settings
/// secrets to `APP_KEY` (from `app.previous_keys`).
pub struct KeyReencrypt;

#[async_trait]
impl AppCommand for KeyReencrypt {
    fn definition(&self) -> Command {
        Command::new("key:reencrypt")
            .about("Seal every encrypted column and settings secret with APP_KEY (after a rotation)")
            .arg(Arg::new("dry-run").long("dry-run").action(ArgAction::SetTrue).help("Check and count; write nothing"))
            .arg(Arg::new("list").long("list").action(ArgAction::SetTrue).help("List the encrypted columns and exit"))
            .arg(
                Arg::new("batch")
                    .long("batch")
                    .value_parser(clap::value_parser!(u64).range(1..=10_000))
                    .default_value("500")
                    .help("Rows read at a time"),
            )
    }

    async fn run(&self, state: &AppState, args: &ArgMatches) -> anyhow::Result<()> {
        let columns = state.service::<EncryptedColumns>().map(|c| c.0.clone()).unwrap_or_default();
        if args.get_flag("list") {
            for column in &columns {
                println!("{}.{} ({:?})", column.table(), column.column, column.binding);
            }
            println!("settings secrets");
            return Ok(());
        }
        let dry_run = args.get_flag("dry-run");
        let batch = *args.get_one::<u64>("batch").expect("defaulted");
        let mut reports: BTreeMap<String, ColumnReport> = BTreeMap::new();
        for column in &columns {
            let report = reencrypt_column(state.db(), state.keyring(), column, batch, dry_run).await?;
            reports.insert(format!("{}.{}", column.table(), column.column), report);
        }
        reports.insert("settings".to_owned(), reencrypt_settings(state, dry_run).await?);
        let mut finished = true;
        let verb = if dry_run { "to move" } else { "moved" };
        for (name, report) in &reports {
            println!(
                "{name}: {} current, {} {verb}, {} changed meanwhile, {} unreadable",
                report.current,
                report.moved,
                report.changed,
                report.unreadable.len()
            );
            for key in &report.unreadable {
                println!("  unreadable: {name} {key}");
            }
            finished &= report.finished(dry_run);
        }
        if !finished {
            return Err(AppError::internal("not finished: run key:reencrypt again (see above)").into());
        }
        Ok(())
    }
}
