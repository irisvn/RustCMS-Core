//! `make:resource` and `make:policy`.

use std::collections::BTreeMap;
use std::process::ExitCode;

use anyhow::{Context, bail};
use serde_json::json;

use crate::generate::{preflight, register, write_new};
use crate::layout;
use crate::naming::{pascal_case, plural, snake_case, validate_model, validate_snake};
use crate::project::Project;
use crate::scaffold::render;

const RESOURCE: &str = include_str!("../templates/stubs/resource.rs.tmpl");
const POLICY: &str = include_str!("../templates/stubs/policy.rs.tmpl");
const ADMIN_PROVIDER: &str = "providers/admin_panel_provider.rs";

/// A column of the model, as written in its `Model` struct.
#[derive(Debug, Clone, PartialEq)]
pub struct Column {
    /// Rust field name (without `r#`).
    pub name: String,
    /// Database column name (`column_name = "…"`, else the field name).
    pub db_name: String,
    pub ty: String,
    pub optional: bool,
    pub primary_key: bool,
    /// The `Column` variant (`enum_name = "…"`, else the field name in PascalCase).
    pub variant: String,
    /// Whether the field is written `r#name`.
    pub raw: bool,
    /// Every `#[…]` attribute of the field, joined.
    pub attrs: String,
}

/// Columns of `pub struct Model { … }`, with their `#[sea_orm(…)]` attributes
/// (`primary_key`, `column_name`, `enum_name`, `ignore`), also when an attribute
/// spans several lines; comments are ignored.
pub fn parse_model(source: &str) -> anyhow::Result<Vec<Column>> {
    let start = source.find("pub struct Model").context("no `pub struct Model` in the model file")?;
    let body = &source[start..];
    let open = body.find('{').context("malformed `struct Model`")?;
    // The matching closing brace (types contain no braces).
    let close = open + body[open..].find('}').context("malformed `struct Model`")?;
    let mut columns = Vec::new();
    let mut attrs = String::new();
    // Brackets still open in the attribute being read.
    let mut depth = 0i32;
    for raw in body[open + 1..close].lines() {
        let line = raw.split("//").next().unwrap_or_default().trim();
        if depth > 0 || line.starts_with("#[") {
            attrs.push_str(line);
            attrs.push(' ');
            depth += line.matches('[').count() as i32 - line.matches(']').count() as i32;
            continue;
        }
        let Some(rest) = line.strip_prefix("pub ") else { continue };
        let Some((name, ty)) = rest.split_once(':') else { continue };
        let raw_ident = name.trim().starts_with("r#");
        let name = name.trim().trim_start_matches("r#").to_owned();
        let ty = ty.trim().trim_end_matches(',').trim();
        let (ty, optional) = match ty.strip_prefix("Option<").and_then(|t| t.strip_suffix('>')) {
            Some(inner) => (inner.trim(), true),
            None => (ty, false),
        };
        // `chrono::NaiveDate` → `NaiveDate`, …
        let ty = ty.rsplit("::").next().unwrap_or(ty).to_owned();
        let taken = std::mem::take(&mut attrs);
        if taken.contains("ignore") && !taken.contains("column_name") {
            continue;
        }
        let db_name = taken
            .split("column_name")
            .nth(1)
            .and_then(|rest| rest.split('"').nth(1))
            .map_or_else(|| name.clone(), str::to_owned);
        let variant = attr_value(&taken, "enum_name").unwrap_or_else(|| pascal_case(&name));
        columns.push(Column {
            name,
            db_name,
            ty,
            optional,
            primary_key: taken.contains("primary_key"),
            variant,
            raw: raw_ident,
            attrs: taken,
        });
    }
    Ok(columns)
}

/// The string value of `key = "…"` in attributes.
pub(crate) fn attr_value(attrs: &str, key: &str) -> Option<String> {
    let rest = attrs.split(key).nth(1)?;
    let rest = rest.trim_start().strip_prefix('=')?;
    rest.split('"').nth(1).map(str::to_owned)
}

const LONG_TEXT: &[&str] = &["body", "content", "description", "notes", "text", "summary", "excerpt", "bio"];
const TIMESTAMPS: &[&str] = &["DateTime", "NaiveDateTime", "DateTimeUtc", "DateTimeWithTimeZone", "DateTimeLocal"];
const MANAGED: &[&str] = &["created_at", "updated_at", "deleted_at"];
/// Never generated as fields (secrets).
const SENSITIVE: &[&str] = &["password", "secret", "token", "hash", "api_key", "salt"];
/// Owners are set by the server (hooks), never by clients.
const OWNERS: &[&str] = &["author_id", "user_id", "owner_id", "created_by", "updated_by"];

/// Rust types the admin engine reads and writes (see `FieldKind`).
fn supported(ty: &str) -> bool {
    matches!(
        ty,
        "String"
            | "i32"
            | "i64"
            | "f32"
            | "f64"
            | "Decimal"
            | "bool"
            | "Uuid"
            | "Date"
            | "NaiveDate"
            | "DateTime"
            | "NaiveDateTime"
            | "DateTimeUtc"
            | "DateTimeWithTimeZone"
            | "DateTimeLocal"
            | "Json"
            | "JsonValue"
            | "Value"
    )
}

struct Generated {
    title: Option<String>,
    key: Option<String>,
    soft_deletes: bool,
    fields: Vec<String>,
    form: Vec<String>,
    columns: Vec<String>,
    filters: Vec<String>,
    warnings: Vec<String>,
}

/// Fields, form, table and filters from the model's columns. Relationships only
/// where `belongs_to` names them (`category_id=categories`); other `*_id`
/// columns get a text input and a TODO.
fn generate(columns: &[Column], belongs_to: &BTreeMap<String, String>) -> Generated {
    let mut out = Generated {
        title: None,
        key: None,
        soft_deletes: false,
        fields: vec![],
        form: vec![],
        columns: vec![],
        filters: vec![],
        warnings: vec![],
    };
    for column in columns {
        let name = &column.db_name;
        // Checks look at both names: `#[sea_orm(column_name)]` may rename either.
        let names = [column.name.as_str(), name.as_str()];
        let col = format!("Column::{}", column.variant);
        if column.primary_key {
            // Database-generated integer keys need nothing; others are declared.
            match column.ty.as_str() {
                "i32" | "i64" => {}
                "String" => {
                    // Natural keys are typed in on create (the edit form omits them).
                    out.key = Some("KeyStrategy::Natural".into());
                    out.fields.push(format!("Field::column({col}).required().searchable().sortable(),"));
                    out.columns.push(format!("TextColumn::make(\"{name}\"),"));
                    out.form.push(format!("TextInput::make(\"{name}\"),"));
                }
                _ => out.warnings.push(format!(
                    "primary key `{name}` ({}): set `.key(KeyStrategy::AppGenerated(…))` in meta() unless the database generates it",
                    column.ty
                )),
            }
            continue;
        }
        if names.iter().any(|n| MANAGED.contains(n)) {
            // A nullable `deleted_at` timestamp (the database name) marks soft deletes.
            let timestamp = TIMESTAMPS.contains(&column.ty.as_str());
            if column.db_name == "deleted_at" && column.optional && timestamp {
                out.soft_deletes = true;
            }
            continue;
        }
        if SENSITIVE.iter().any(|s| names.iter().any(|n| n.contains(s))) {
            out.warnings.push(format!("`{name}` looks secret: not exposed (add it by hand if needed, as write-only)"));
            continue;
        }
        if !supported(&column.ty) {
            out.warnings.push(format!("`{name}`: type `{}` is not supported by the admin panel; skipped", column.ty));
            continue;
        }
        let mut field = format!("Field::column({col})");
        if names.iter().any(|n| OWNERS.contains(n)) && !belongs_to.contains_key(name) {
            // Shown, never written by clients: set it in `mutate_before_create`.
            out.fields
                .push(format!("{field}.guarded().readable(|_| true), // set by the server (mutate_before_create)"));
            continue;
        }
        let required = !column.optional && column.ty != "bool";
        if required {
            field.push_str(".required()");
        }
        let input = match (column.ty.as_str(), belongs_to.get(name)) {
            (_, Some(slug)) => {
                field.push_str(&format!(".belongs_to(\"{slug}\").filterable()"));
                out.columns.push(format!("TextColumn::make(\"{name}\"),"));
                out.filters.push(format!("SelectFilter::make(\"{name}\"),"));
                format!("SelectInput::make(\"{name}\"),")
            }
            ("String", _) if LONG_TEXT.contains(&name.as_str()) => {
                format!("Textarea::make(\"{name}\").column_span(2),")
            }
            ("String", _) => {
                if out.title.is_none() {
                    out.title = Some(name.clone());
                    field.push_str(".searchable().sortable()");
                    out.columns.push(format!("TextColumn::make(\"{name}\"),"));
                }
                format!("TextInput::make(\"{name}\"),")
            }
            ("bool", _) => {
                field.push_str(".filterable()");
                out.columns.push(format!("BooleanColumn::make(\"{name}\"),"));
                out.filters.push(format!("TernaryFilter::make(\"{name}\"),"));
                format!("Toggle::make(\"{name}\"),")
            }
            ("i32" | "i64" | "f32" | "f64" | "Decimal", _) if name.ends_with("_id") => {
                out.warnings.push(format!(
                    "`{name}` looks like a relationship: pass `--belongs-to {name}=<slug>` to get a select"
                ));
                format!("TextInput::make(\"{name}\"), // TODO(rustcms): relationship? `--belongs-to {name}=<slug>`")
            }
            ("i32" | "i64" | "f32" | "f64" | "Decimal", _) => {
                field.push_str(".sortable()");
                format!("NumberInput::make(\"{name}\"),")
            }
            ("Date" | "NaiveDate", _) => {
                field.push_str(".sortable().filterable()");
                out.columns.push(format!("DateColumn::make(\"{name}\"),"));
                out.filters.push(format!("DateRangeFilter::make(\"{name}\"),"));
                format!("DatePicker::make(\"{name}\"),")
            }
            ("DateTime" | "NaiveDateTime" | "DateTimeUtc" | "DateTimeWithTimeZone" | "DateTimeLocal", _) => {
                field.push_str(".sortable()");
                out.columns.push(format!("DateColumn::make(\"{name}\"),"));
                format!("DateTimePicker::make(\"{name}\"),")
            }
            ("Json" | "JsonValue" | "Value", _) => format!("Textarea::make(\"{name}\"), // JSON"),
            _ => format!("TextInput::make(\"{name}\"),"),
        };
        out.fields.push(format!("{field},"));
        out.form.push(input);
    }
    out
}

pub fn make_resource(
    project: &Project,
    model: &str,
    generate_fields: bool,
    belongs_to: &[String],
) -> anyhow::Result<ExitCode> {
    validate_model(model)?;
    let module = snake_case(model);
    let table = plural(&module);
    let model_file = project.root.join("src/models").join(format!("{module}.rs"));
    if !model_file.exists() {
        bail!("{} not found: create the model first (`rustcms make:model {model} -m`)", model_file.display());
    }
    if !belongs_to.is_empty() && !generate_fields {
        bail!("--belongs-to needs --generate");
    }
    let mut relations = BTreeMap::new();
    for spec in belongs_to {
        let Some((field, slug)) = spec.split_once('=') else {
            bail!("--belongs-to expects `field=slug`, e.g. `category_id=categories`");
        };
        let (field, slug) = (field.trim(), slug.trim());
        validate_snake(field)?;
        validate_snake(slug)?;
        if relations.insert(field.to_owned(), slug.to_owned()).is_some() {
            bail!("--belongs-to: `{field}` given twice");
        }
    }
    let generated = if generate_fields {
        let columns = parse_model(&std::fs::read_to_string(&model_file)?)?;
        for field in relations.keys() {
            match columns.iter().find(|c| &c.db_name == field) {
                None => bail!("--belongs-to: `{field}` is not a column of {model}"),
                Some(c) if c.primary_key || MANAGED.contains(&field.as_str()) => {
                    bail!("--belongs-to: `{field}` cannot be a relationship")
                }
                Some(_) => {}
            }
        }
        generate(&columns, &relations)
    } else {
        Generated {
            title: None,
            key: None,
            soft_deletes: false,
            fields: vec!["// Field::column(Column::Title).required().searchable(),".into()],
            form: vec![],
            columns: vec![],
            filters: vec![],
            warnings: vec![],
        }
    };
    let file = project.src(layout::RESOURCES.path).join(format!("{module}_resource.rs"));
    let provider = project.src(ADMIN_PROVIDER);
    preflight(&[&file], &[(&provider, "// rustcms:resources")])?;
    let index = project.prepare_module(&layout::RESOURCES)?;
    let fields: Vec<String> =
        generated.fields.iter().map(|f| f.replace("Column::", &format!("{module}::Column::"))).collect();
    let contents = render(
        RESOURCE,
        json!({
            "name": model,
            "module": module,
            "table": table,
            "label": humanize(model),
            "plural_label": humanize_plural(&table),
            "title": generated.title,
            "key": generated.key,
            "soft_deletes": generated.soft_deletes,
            "fields": fields,
            "form": generated.form,
            "columns": generated.columns,
            "filters": generated.filters,
        }),
    )?;
    write_new(&file, &contents)?;
    register(&index, layout::RESOURCES.anchor, &format!("pub mod {module}_resource;"))?;
    let wired = register(
        &provider,
        "// rustcms:resources",
        &format!(".resource::<crate::admin::resources::{module}_resource::{model}Resource>()"),
    )?;
    for warning in &generated.warnings {
        println!("warning: {warning}");
    }
    if wired {
        println!("Permissions `{table}.view_any`, `{table}.create`, … are declared; run `rustcms permission:sync`.");
    }
    Ok(ExitCode::SUCCESS)
}

pub fn make_policy(project: &Project, name: &str) -> anyhow::Result<ExitCode> {
    validate_model(name)?;
    let Some(stem) = name.strip_suffix("Policy").filter(|s| !s.is_empty()) else {
        bail!("policy names end with `Policy`, e.g. `PostPolicy`");
    };
    let module = snake_case(name);
    let slug = plural(&snake_case(stem));
    let contents = render(POLICY, json!({ "name": name, "slug": slug }))?;
    let file = project.src(layout::POLICIES.path).join(format!("{module}.rs"));
    preflight(&[&file], &[])?;
    let index = project.prepare_module(&layout::POLICIES)?;
    write_new(&file, &contents)?;
    register(&index, layout::POLICIES.anchor, &format!("pub mod {module};"))?;
    println!(
        "Use it in a resource: `fn policy() -> std::sync::Arc<dyn Policy> {{ std::sync::Arc::new(crate::policies::{module}::{name}) }}`"
    );
    Ok(ExitCode::SUCCESS)
}

/// `blog_posts` → `Blog posts`.
fn humanize_plural(table: &str) -> String {
    let words = table.replace('_', " ");
    let mut chars = words.chars();
    chars.next().map(|c| c.to_ascii_uppercase().to_string() + chars.as_str()).unwrap_or_default()
}

/// `BlogPost` → `Blog post`.
fn humanize(model: &str) -> String {
    let words = snake_case(model).replace('_', " ");
    let mut chars = words.chars();
    chars.next().map(|c| c.to_ascii_uppercase().to_string() + chars.as_str()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    const POST: &str = r#"
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "posts")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    pub title: String,
    pub body: Option<String>, // long text
    pub category_id: Option<i32>,
    pub author_id: i64,
    pub published: bool,
    pub published_at: Option<DateTimeUtc>,
    pub views: i32,
    pub r#type: String,
    #[sea_orm(column_name = "full_name")]
    pub display: String,
    #[sea_orm(ignore)]
    pub cached: String,
    pub stock: u64,
    pub password_hash: String,
    pub created_at: DateTimeUtc,
    pub updated_at: DateTimeUtc,
}

impl ActiveModelBehavior for ActiveModel {}
"#;

    fn generated() -> Generated {
        let columns = parse_model(POST).unwrap();
        generate(&columns, &BTreeMap::from([("category_id".into(), "categories".into())]))
    }

    #[test]
    fn parses_columns_attributes_and_comments() {
        let columns = parse_model(POST).unwrap();
        let names: Vec<&str> = columns.iter().map(|c| c.db_name.as_str()).collect();
        assert!(names.contains(&"type") && names.contains(&"full_name") && !names.contains(&"cached"), "{names:?}");
        let body = columns.iter().find(|c| c.name == "body").unwrap();
        assert_eq!((body.ty.as_str(), body.optional), ("String", true));
        assert!(columns.iter().find(|c| c.name == "id").unwrap().primary_key);
    }

    #[test]
    fn generates_inputs_by_type() {
        let g = generated();
        let form = g.form.join("\n");
        assert_eq!(g.title.as_deref(), Some("title"));
        for expected in [
            "TextInput::make(\"title\")",
            "Textarea::make(\"body\")",
            "SelectInput::make(\"category_id\")",
            "Toggle::make(\"published\")",
            "DateTimePicker::make(\"published_at\")",
            "NumberInput::make(\"views\")",
            "TextInput::make(\"type\")",
            "TextInput::make(\"full_name\")",
        ] {
            assert!(form.contains(expected), "{expected} in {form}");
        }
        let fields = g.fields.join("\n");
        assert!(fields.contains("Column::Type"), "raw identifiers lose `r#`: {fields}");
        assert!(fields.contains("Column::Display"), "renamed columns keep their variant");
        assert!(fields.contains("Column::Title).required()"));
        assert!(fields.contains("Column::Views).required()"), "NOT NULL columns are required");
        assert!(fields.contains(".belongs_to(\"categories\")"));
        assert!(
            g.columns.iter().any(|c| c.contains("category_id")) && g.filters.iter().any(|f| f.contains("category_id"))
        );
    }

    #[test]
    fn skips_what_must_not_be_generated() {
        let g = generated();
        let all = format!("{}\n{}", g.fields.join("\n"), g.form.join("\n"));
        assert!(!all.contains("cached") && !all.contains("created_at") && !all.contains("Column::Id"));
        assert!(!all.contains("password_hash"), "secrets are never exposed");
        assert!(!all.contains("\"stock\""), "unsupported types (u64) are skipped");
        assert!(g.fields.iter().any(|f| f.contains("Column::AuthorId).guarded()")), "owners are set by the server");
        assert!(!g.form.iter().any(|f| f.contains("author_id")));
        assert!(
            g.warnings.iter().any(|w| w.contains("stock")) && g.warnings.iter().any(|w| w.contains("password_hash"))
        );
    }

    #[test]
    fn natural_string_keys_are_declared() {
        let model = "pub struct Model {\n    #[sea_orm(primary_key, auto_increment = false)]\n    pub slug: String,\n    pub label: String,\n}\n";
        let g = generate(&parse_model(model).unwrap(), &BTreeMap::new());
        assert_eq!(g.key.as_deref(), Some("KeyStrategy::Natural"));
        assert!(g.form.iter().any(|f| f.contains("TextInput::make(\"slug\")")), "the key is typed in: {:?}", g.form);
        assert!(g.fields.iter().any(|f| f.contains("Column::Slug).required()")), "{:?}", g.fields);
        assert_eq!(g.title.as_deref(), Some("label"), "the title stays a non-key column");
    }

    #[test]
    fn nullable_deleted_at_means_soft_deletes() {
        let model = |ty: &str| {
            format!(
                "pub struct Model {{\n    #[sea_orm(primary_key)]\n    pub id: i32,\n    pub title: String,\n    pub deleted_at: {ty},\n}}\n"
            )
        };
        let soft = |ty: &str| generate(&parse_model(&model(ty)).unwrap(), &BTreeMap::new()).soft_deletes;
        assert!(soft("Option<DateTimeUtc>"));
        assert!(soft("Option<chrono::NaiveDateTime>"));
        assert!(!soft("DateTimeUtc"), "not nullable");
        assert!(!soft("Option<bool>"), "not a timestamp");
        let g = generate(&parse_model(&model("Option<DateTimeUtc>")).unwrap(), &BTreeMap::new());
        assert!(!g.fields.iter().any(|f| f.contains("DeletedAt")), "no field for the marker");
    }

    #[test]
    fn attributes_over_several_lines_are_read_whole() {
        let model = "pub struct Model {\n    #[sea_orm(\n        primary_key,\n        enum_name = \"Key\"\n    )]\n    pub id: i32,\n    #[sea_orm(\n        column_name = \"api_key\"\n    )]\n    pub credential: String,\n    pub r#type: String,\n}\n";
        let columns = parse_model(model).unwrap();
        assert!(columns[0].primary_key && columns[0].variant == "Key");
        assert_eq!(columns[1].db_name, "api_key");
        assert!(columns[2].raw && columns[2].name == "type");
        // The secret is not generated as a field.
        assert!(!generate(&columns, &BTreeMap::new()).fields.iter().any(|f| f.contains("Credential")));
    }

    #[test]
    fn renamed_columns_are_checked_under_both_names() {
        let model = "pub struct Model {\n    #[sea_orm(primary_key)]\n    pub id: i32,\n    pub title: String,\n    #[sea_orm(column_name = \"pw\")]\n    pub password_hash: String,\n    #[sea_orm(column_name = \"api_key\")]\n    pub credential: String,\n    #[sea_orm(column_name = \"writer\")]\n    pub author_id: i64,\n    #[sea_orm(column_name = \"user_id\")]\n    pub writer: i64,\n}\n";
        let g = generate(&parse_model(model).unwrap(), &BTreeMap::new());
        let all = format!("{}\n{}", g.fields.join("\n"), g.form.join("\n"));
        assert!(!all.contains("PasswordHash") && !all.contains("\"pw\""), "{all}");
        assert!(!all.contains("Credential") && !all.contains("api_key"), "{all}");
        assert!(g.fields.iter().any(|f| f.contains("Column::AuthorId).guarded()")), "{all}");
        assert!(g.fields.iter().any(|f| f.contains("Column::Writer).guarded()")), "{all}");
        assert!(!g.form.iter().any(|f| f.contains("writer") || f.contains("user_id")), "{all}");
    }

    fn project() -> (tempfile::TempDir, Project) {
        let dir = tempfile::tempdir().unwrap();
        for (path, contents) in [
            ("src/models/post.rs", POST),
            (
                "src/providers/admin_panel_provider.rs",
                "fn provider() {\n    AdminProvider::new()\n        // rustcms:resources\n}\n",
            ),
            ("src/lib.rs", "pub mod providers;\n// rustcms:mods\n"),
        ] {
            let file = dir.path().join(path);
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(file, contents).unwrap();
        }
        let project = Project { root: dir.path().to_path_buf(), package: "blog".into() };
        (dir, project)
    }

    #[test]
    fn writes_registers_once_and_validates_input() {
        let (dir, project) = project();
        make_resource(&project, "Post", true, &["category_id=categories".into()]).unwrap();
        let provider = || std::fs::read_to_string(dir.path().join("src/providers/admin_panel_provider.rs")).unwrap();
        assert_eq!(
            provider().matches(".resource::<crate::admin::resources::post_resource::PostResource>()").count(),
            1
        );
        // The module chain is created: lib.rs → admin → resources.
        assert!(std::fs::read_to_string(dir.path().join("src/lib.rs")).unwrap().contains("pub mod admin;"));
        assert!(std::fs::read_to_string(dir.path().join("src/admin/mod.rs")).unwrap().contains("pub mod resources;"));
        let file = dir.path().join("src/admin/resources/post_resource.rs");
        assert!(std::fs::read_to_string(&file).unwrap().contains(".plural(\"Posts\")"));
        // Regenerating after deleting the file does not register twice.
        std::fs::remove_file(&file).unwrap();
        make_resource(&project, "Post", true, &[]).unwrap();
        assert_eq!(provider().matches("PostResource").count(), 1);
        let index = std::fs::read_to_string(dir.path().join("src/admin/resources/mod.rs")).unwrap();
        assert_eq!(index.matches("pub mod post_resource;").count(), 1);
        std::fs::remove_file(&file).unwrap();
        for bad in ["category_id", "category_id=cat\"egories", "category_id=", "id=x", "nope=x"] {
            assert!(make_resource(&project, "Post", true, &[bad.into()]).is_err(), "{bad}");
        }
        assert!(
            make_resource(&project, "Post", false, &["category_id=categories".into()]).is_err(),
            "needs --generate"
        );
        assert!(make_resource(&project, "Missing", true, &[]).is_err());
    }

    #[test]
    fn policy_names() {
        let (dir, project) = project();
        make_policy(&project, "PostPolicy").unwrap();
        let policy = std::fs::read_to_string(dir.path().join("src/policies/post_policy.rs")).unwrap();
        assert!(policy.contains("format!(\"posts.{ability}\")") && policy.contains("use rustcms::async_trait;"));
        for bad in ["Policy", "Post", "bad"] {
            assert!(make_policy(&project, bad).is_err(), "{bad}");
        }
        assert!(make_resource(&project, "Type", false, &[]).is_err(), "keyword module");
    }
}
