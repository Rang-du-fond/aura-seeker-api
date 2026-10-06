use std::collections::HashMap;

use async_trait::async_trait;
use chrono::Utc;
use sea_orm::{
    ConnectionTrait, Database, DatabaseConnection, DatabaseTransaction, DbErr, JsonValue as Json, QueryResult, SqlErr,
    StatementBuilder, TransactionTrait,
    sea_query::{Alias, Asterisk, Cond, Expr, ExprTrait, Func, OnConflict, Query, SimpleExpr, Value},
};
use uuid::Uuid;

use super::{Condition, Record, Repository, Stored};
use crate::error::{Error, Result};

const MIGRATIONS_TABLE: &str =
    "CREATE TABLE IF NOT EXISTS migrations (name TEXT PRIMARY KEY, applied_at TEXT NOT NULL)";

pub struct Migration {
    pub name: &'static str,
    pub sql: &'static str,
}

impl From<DbErr> for Error {
    fn from(cause: DbErr) -> Self {
        match cause.sql_err() {
            Some(SqlErr::UniqueConstraintViolation(_)) => {
                Self::Conflict("a resource with the same unique value already exists")
            }
            Some(SqlErr::ForeignKeyConstraintViolation(_)) => {
                Self::Conflict("a referenced resource does not exist, or this resource is still referenced")
            }
            _ => Self::unexpected(cause),
        }
    }
}

const LIST_OWNER: &str = "resource_id";
const LIST_ITEM: &str = "value";

type Document = serde_json::Map<String, Json>;
type Columns = Vec<(Alias, SimpleExpr)>;
type Lists = Vec<(&'static str, Vec<Json>)>;

pub trait SqlConnection: ConnectionTrait + TransactionTrait<Transaction = DatabaseTransaction> + Send + Sync {}

impl<C: ConnectionTrait + TransactionTrait<Transaction = DatabaseTransaction> + Send + Sync> SqlConnection for C {}

pub type SqlTransaction = SqlRepository<DatabaseTransaction>;

#[derive(Clone)]
pub struct SqlRepository<C = DatabaseConnection>(C);

impl SqlRepository {
    pub async fn connect(url: &str, migrations: &[Migration]) -> Result<Self> {
        let repository = Self(Database::connect(url).await?);
        repository.0.execute_unprepared(MIGRATIONS_TABLE).await?;
        let applied = repository.applied_migrations().await?;
        for migration in migrations.iter().filter(|migration| !applied.iter().any(|name| name == migration.name)) {
            repository.apply(migration).await?;
        }
        Ok(repository)
    }

    async fn applied_migrations(&self) -> Result<Vec<String>> {
        let mut query = Query::select();
        query.column(Alias::new("name")).from(Alias::new("migrations"));
        let rows = self.0.query_all(&query).await?;
        Ok(rows.iter().map(|row| row.try_get("", "name")).collect::<std::result::Result<_, _>>()?)
    }

    async fn apply(&self, migration: &Migration) -> Result<()> {
        let mut record = Query::insert();
        record
            .into_table(Alias::new("migrations"))
            .columns([Alias::new("name"), Alias::new("applied_at")])
            .values([migration.name.into(), Utc::now().to_rfc3339().into()])
            .map_err(Error::unexpected)?;
        let transaction = self.0.begin().await?;
        transaction.execute_unprepared(migration.sql).await?;
        transaction.execute(&record).await?;
        Ok(transaction.commit().await?)
    }
}

impl SqlRepository {
    pub async fn is_reachable(&self) -> bool {
        self.0.ping().await.is_ok()
    }
}

impl SqlTransaction {
    pub async fn committed(self) -> Result<()> {
        Ok(self.0.commit().await?)
    }
}

impl<C: SqlConnection> SqlRepository<C> {
    pub async fn transaction(&self) -> Result<SqlTransaction> {
        Ok(SqlRepository(self.0.begin().await?))
    }

    #[tracing::instrument(name = "sql.update_where", skip_all, fields(table = T::COLLECTION))]
    pub async fn update_where<T: Record>(&self, stored: &Stored<T>, conditions: &[Condition]) -> Result<()> {
        let (columns, lists) = fields(stored)?;
        let unchanged = conditions.iter().fold(Cond::all(), |all, condition| all.add(predicate::<T>(condition)));
        let mut query = Query::update();
        query.table(table::<T>()).values(columns).and_where(identified_by(stored.id)).cond_where(unchanged);
        self.write::<T>(stored.id, &query, lists).await
    }

    #[tracing::instrument(name = "sql.delete_where", skip_all, fields(table = T::COLLECTION))]
    pub async fn delete_where<T: Record>(&self, conditions: &[Condition]) -> Result<u64> {
        let matching = conditions.iter().fold(Cond::all(), |all, condition| all.add(predicate::<T>(condition)));
        let mut query = Query::delete();
        query.from_table(table::<T>()).cond_where(matching);
        Ok(self.0.execute(&query).await?.rows_affected())
    }

    #[tracing::instrument(name = "sql.select", skip_all, fields(table = T::COLLECTION))]
    pub async fn select<T: Record>(&self, conditions: &[Condition]) -> Result<Vec<Stored<T>>> {
        let mut query = Query::select();
        query
            .column(Asterisk)
            .from(table::<T>())
            .cond_where(conditions.iter().fold(Cond::all(), |all, condition| all.add(predicate::<T>(condition))));
        let mut documents: Vec<Document> = self.0.query_all(&query).await?.iter().map(document).collect();
        for list in T::LISTS {
            self.attach_list::<T>(list, &mut documents).await?;
        }
        documents
            .into_iter()
            .map(|document| serde_json::from_value(document.into()).map_err(Error::unexpected))
            .collect()
    }

    async fn attach_list<T: Record>(&self, list: &str, owners: &mut [Document]) -> Result<()> {
        let owner_ids = owners.iter().map(|owner| text(owner, "id"));
        let mut query = Query::select();
        query
            .column(Asterisk)
            .from(list_table::<T>(list))
            .and_where(Expr::col(Alias::new(LIST_OWNER)).is_in(owner_ids));
        let mut items = HashMap::<String, Vec<Json>>::new();
        for row in self.0.query_all(&query).await?.iter().map(document) {
            items.entry(text(&row, LIST_OWNER).to_owned()).or_default().extend(row.get(LIST_ITEM).cloned());
        }
        for owner in owners {
            let owned = items.remove(text(owner, "id")).unwrap_or_default();
            owner.insert(list.to_owned(), owned.into());
        }
        Ok(())
    }

    #[tracing::instrument(name = "sql.write", skip_all, fields(table = T::COLLECTION))]
    async fn write<T: Record>(&self, id: Uuid, statement: &impl StatementBuilder, lists: Lists) -> Result<()> {
        let transaction = self.0.begin().await?;
        affect_one(&transaction, statement).await?;
        for (list, items) in lists {
            replace_list(&transaction, list_table::<T>(list), id, items).await?;
        }
        Ok(transaction.commit().await?)
    }
}

async fn replace_list(connection: &impl ConnectionTrait, table: Alias, owner: Uuid, items: Vec<Json>) -> Result<()> {
    let mut clear = Query::delete();
    clear.from_table(table.clone()).and_where(equals(LIST_OWNER, owner.to_string()));
    connection.execute(&clear).await?;
    if items.is_empty() {
        return Ok(());
    }
    let mut fill = Query::insert();
    fill.into_table(table).columns([Alias::new(LIST_OWNER), Alias::new(LIST_ITEM)]);
    for item in items {
        fill.values([owner.to_string().into(), sql_value(item).into()]).map_err(Error::unexpected)?;
    }
    connection.execute(&fill).await?;
    Ok(())
}

async fn affect_one(connection: &impl ConnectionTrait, statement: &impl StatementBuilder) -> Result<()> {
    match connection.execute(statement).await?.rows_affected() {
        0 => Err(Error::NotFound),
        _ => Ok(()),
    }
}

#[async_trait]
impl<T: Record, C: SqlConnection> Repository<T> for SqlRepository<C> {
    async fn search(&self, conditions: &[Condition]) -> Result<Vec<Stored<T>>> {
        self.select(conditions).await
    }

    async fn find(&self, id: Uuid) -> Result<Stored<T>> {
        self.select(&[Condition::Equals("id", id.to_string().into())]).await?.pop().ok_or(Error::NotFound)
    }

    async fn insert(&self, stored: &Stored<T>) -> Result<()> {
        let (columns, lists) = fields(stored)?;
        let (columns, values): (Vec<_>, Vec<_>) = columns.into_iter().unzip();
        let mut query = Query::insert();
        query.into_table(table::<T>()).columns(columns).values(values).map_err(Error::unexpected)?;
        self.write::<T>(stored.id, &query, lists).await
    }

    async fn update(&self, stored: &Stored<T>) -> Result<()> {
        self.update_where(stored, &[]).await
    }

    async fn delete(&self, id: Uuid) -> Result<()> {
        let mut query = Query::delete();
        query.from_table(table::<T>()).and_where(identified_by(id));
        affect_one(&self.0, &query).await
    }

    async fn add_to_list(&self, id: Uuid, list: &'static str, item: Json) -> Result<()> {
        let columns = [Alias::new(LIST_OWNER), Alias::new(LIST_ITEM)];
        let mut query = Query::insert();
        query
            .into_table(list_table::<T>(list))
            .columns(columns.clone())
            .values([id.to_string().into(), sql_value(item).into()])
            .map_err(Error::unexpected)?
            .on_conflict(OnConflict::columns(columns).do_nothing().to_owned());
        self.0.execute(&query).await?;
        Ok(())
    }

    async fn remove_from_list(&self, id: Uuid, list: &'static str, item: Json) -> Result<()> {
        let mut query = Query::delete();
        query
            .from_table(list_table::<T>(list))
            .and_where(equals(LIST_OWNER, id.to_string()))
            .and_where(equals(LIST_ITEM, sql_value(item)));
        self.0.execute(&query).await?;
        Ok(())
    }
}

fn table<T: Record>() -> Alias {
    Alias::new(T::COLLECTION)
}

fn list_table<T: Record>(list: &str) -> Alias {
    Alias::new(format!("{}_{list}", T::COLLECTION))
}

fn equals(column: &str, value: impl Into<Value>) -> SimpleExpr {
    Expr::col(Alias::new(column)).eq(value.into())
}

fn predicate<T: Record>(condition: &Condition) -> SimpleExpr {
    let column = |field: &str| Expr::col(Alias::new(field));
    match condition {
        Condition::Equals(field, value) => column(field).eq(sql_value(value.clone())),
        Condition::GreaterThan(field, value) => column(field).gt(sql_value(value.clone())),
        Condition::LessThan(field, value) => column(field).lt(sql_value(value.clone())),
        Condition::Missing(field) => column(field).is_null(),
        Condition::Contains(field, text) => {
            Expr::expr(Func::lower(column(field))).like(format!("%{}%", text.to_lowercase()))
        }
        Condition::Includes(list, item) => {
            let mut owners = Query::select();
            owners
                .column(Alias::new(LIST_OWNER))
                .from(list_table::<T>(list))
                .and_where(equals(LIST_ITEM, sql_value(item.clone())));
            column("id").in_subquery(owners)
        }
    }
}

fn identified_by(id: Uuid) -> SimpleExpr {
    equals("id", id.to_string())
}

fn fields<T: Record>(stored: &Stored<T>) -> Result<(Columns, Lists)> {
    let fields = serde_json::to_value(stored).map_err(Error::unexpected)?;
    let mut fields: Document = serde_json::from_value(fields).map_err(Error::unexpected)?;
    let mut items = |list| fields.remove(list).and_then(|items| items.as_array().cloned()).unwrap_or_default();
    let lists = T::LISTS.iter().map(|list| (*list, items(*list))).collect();
    Ok((fields.into_iter().map(|(name, value)| (Alias::new(name), sql_value(value).into())).collect(), lists))
}

fn text<'document>(document: &'document Document, field: &str) -> &'document str {
    document.get(field).and_then(Json::as_str).unwrap_or_default()
}

fn document(row: &QueryResult) -> Document {
    row.column_names().into_iter().map(|column| (column.clone(), json_value(row, &column))).collect()
}

fn json_value(row: &QueryResult, column: &str) -> Json {
    row.try_get::<Option<i64>>("", column)
        .map(Json::from)
        .or_else(|_| row.try_get::<Option<f64>>("", column).map(Json::from))
        .or_else(|_| row.try_get::<Option<bool>>("", column).map(Json::from))
        .or_else(|_| row.try_get::<Option<String>>("", column).map(Json::from))
        .unwrap_or_default()
}

fn sql_value(json: Json) -> Value {
    match json {
        Json::Null => Value::String(None),
        Json::Bool(boolean) => boolean.into(),
        Json::Number(number) => number.as_i64().map_or_else(|| number.as_f64().into(), Value::from),
        Json::String(text) => text.into(),
        nested @ (Json::Array(_) | Json::Object(_)) => nested.to_string().into(),
    }
}
