//! `make:api`: a JSON API module for a model (`src/http/api/<table>/`).

use std::process::ExitCode;

use anyhow::{Context, bail};
use serde_json::json;

use crate::generate::{register, write_new};
use crate::naming::{plural, snake_case, validate_model};
use crate::project::Project;
use crate::resource::{Column, attr_value, parse_model};
use crate::scaffold::render;

const MOD: &str = include_str!("../templates/stubs/api_mod.rs.tmpl");
const HANDLERS: &str = include_str!("../templates/stubs/api_handlers.rs.tmpl");
const JSON: &str = include_str!("../templates/stubs/api_json.rs.tmpl");

const MODS_ANCHOR: &str = "// rustcms:api-mods";
const ROUTES_ANCHOR: &str = "// rustcms:api-routes";
/// Never exposed by a generated API (secrets).
const SENSITIVE: &[&str] = &["password", "secret", "token", "hash", "api_key", "salt"];
/// Rust keywords: such names are written `r#name`, or get a suffix as module names.
const KEYWORDS: &[&str] = &[
    "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum", "extern", "false", "fn",
    "for", "gen", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub", "ref", "return", "self",
    "static", "struct", "super", "trait", "true", "try", "type", "unsafe", "use", "where", "while", "yield",
];

/// `name` as a Rust identifier (`r#type`).
fn ident(name: &str) -> String {
    if KEYWORDS.contains(&name) { format!("r#{name}") } else { name.to_owned() }
}

/// A module name for a table: `blog-posts` → `blog_posts`, `type` → `type_`.
fn module_ident(table: &str) -> String {
    let mut out: String =
        table.chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '_' }).collect();
    if out.is_empty() || out.starts_with(|c: char| c.is_ascii_digit()) {
        out.insert_str(0, "t_");
    }
    if KEYWORDS.contains(&out.as_str()) {
        out.push('_');
    }
    out
}

/// The scale of `column_type = "Decimal(Some((p, s)))"`, if declared.
fn decimal_scale(attrs: &str) -> Option<u32> {
    let decl = attr_value(attrs, "column_type")?;
    let inner = decl.strip_prefix("Decimal(Some((")?;
    inner.split(',').nth(1)?.trim().trim_end_matches(')').trim().parse().ok()
}

/// A field of the generated JSON: its type and how it is read from the model.
#[derive(Debug, PartialEq)]
struct JsonField {
    name: String,
    ty: String,
    expr: String,
}

/// How a column is written in JSON (ADR 0003): 64-bit integers, decimals,
/// UUIDs and times as strings; `None` for types the API leaves out.
fn json_field(column: &Column) -> Option<JsonField> {
    // `{}` stands for the value.
    let decimal = match decimal_scale(&column.attrs) {
        Some(scale) => format!("format!(\"{{:.{scale}}}\", {{}})"),
        None => "{}.to_string()".to_owned(),
    };
    let (ty, convert): (&str, Option<&str>) = match column.ty.as_str() {
        "String" | "bool" | "i32" | "i16" | "i8" | "u8" | "u16" | "f32" | "f64" => (column.ty.as_str(), None),
        "i64" | "u64" | "u32" | "Uuid" | "Date" | "NaiveDate" => ("String", Some("{}.to_string()")),
        "Decimal" => ("String", Some(decimal.as_str())),
        // Instants in UTC (ADR 0003), whatever offset they were stored with.
        "DateTimeUtc" | "DateTimeWithTimeZone" => {
            ("String", Some("{}.naive_utc().format(\"%Y-%m-%dT%H:%M:%S%.fZ\").to_string()"))
        }
        "DateTime" | "NaiveDateTime" => ("String", Some("{}.format(\"%Y-%m-%dT%H:%M:%S%.f\").to_string()")),
        "Json" | "JsonValue" | "Value" => ("serde_json::Value", None),
        _ => return None,
    };
    let name = if column.raw { format!("r#{}", column.name) } else { ident(&column.name) };
    let field = format!("m.{name}");
    let (ty, expr) = match (column.optional, convert) {
        (false, None) => (ty.to_owned(), field),
        (false, Some(c)) => (ty.to_owned(), c.replace("{}", &field)),
        (true, None) => (format!("Option<{ty}>"), field),
        (true, Some(c)) => (format!("Option<{ty}>"), format!("{field}.map(|v| {})", c.replace("{}", "v"))),
    };
    Some(JsonField { name, ty, expr })
}

/// `table_name = "…"` of the model's `#[sea_orm(…)]` attribute (comments
/// ignored), else the plural of its module.
fn table_of(source: &str, module: &str) -> String {
    let mut attrs = String::new();
    for line in source.lines() {
        let line = line.split("//").next().unwrap_or_default().trim();
        if line.starts_with("pub struct Model") {
            break;
        }
        if line.starts_with("#[") || (!attrs.is_empty() && !line.is_empty()) {
            attrs.push_str(line);
            attrs.push(' ');
        }
    }
    attr_value(&attrs, "table_name").unwrap_or_else(|| plural(module))
}

pub fn make_api(project: &Project, model: &str) -> anyhow::Result<ExitCode> {
    validate_model(model)?;
    let module = snake_case(model);
    let model_file = project.root.join("src/models").join(format!("{module}.rs"));
    if !model_file.exists() {
        bail!("{} not found: create the model first (`rustcms make:model {model} -m`)", model_file.display());
    }
    let source = std::fs::read_to_string(&model_file).with_context(|| format!("reading {}", model_file.display()))?;
    let columns = parse_model(&source)?;
    let keys: Vec<&Column> = columns.iter().filter(|c| c.primary_key).collect();
    let key = match keys.as_slice() {
        [key] => *key,
        [] => bail!("`{model}` has no primary key"),
        _ => bail!("`{model}` has a composite primary key: write its API by hand"),
    };
    let key_type = match key.ty.as_str() {
        "i8" | "i16" | "i32" | "i64" | "u8" | "u16" | "u32" | "u64" | "String" => key.ty.clone(),
        "Uuid" => "sea_orm::prelude::Uuid".to_owned(),
        other => bail!("`{model}`: primary key type `{other}` is not supported by make:api"),
    };
    let table = table_of(&source, &module);
    // Check everything before writing anything.
    let api_dir = project.root.join("src/http/api");
    let index = api_dir.join("mod.rs");
    let index_source = std::fs::read_to_string(&index)
        .with_context(|| format!("{} not found (apps created before 0.2: see the API guide)", index.display()))?;
    for anchor in [MODS_ANCHOR, ROUTES_ANCHOR] {
        if !index_source.lines().any(|l| l.trim() == anchor) {
            bail!("{} has no `{anchor}` line: add it where `make:api` should register modules", index.display());
        }
    }
    // The URL keeps the table name; the module needs a Rust identifier.
    let module_name = module_ident(&table);
    let dir = api_dir.join(&module_name);
    if dir.exists() {
        bail!("{} already exists", dir.display());
    }

    let mut fields = Vec::new();
    for column in &columns {
        if SENSITIVE.iter().any(|s| column.name.contains(s) || column.db_name.contains(s)) {
            println!("warning: `{}` looks secret: not exposed", column.name);
            continue;
        }
        match json_field(column) {
            Some(field) => fields.push(field),
            None => println!("warning: `{}` ({}): type not supported by make:api; left out", column.name, column.ty),
        }
    }
    let label = crate::naming::pascal_case(&table);
    let vars = json!({
        "model": model,
        "module": module,
        "table": table,
        "label": label,
        "label_lower": module.replace('_', " "),
        "key_column": key.variant,
        "module_name": module_name,
        // Newest first when the model has a creation time, else by key.
        "created_column": columns
            .iter()
            .find(|c| c.db_name == "created_at" && !c.optional && c.ty.starts_with("DateTime"))
            .map(|c| c.variant.clone()),
        "key_type": key_type,
        "fields": fields.iter().map(|f| json!({"name": f.name, "ty": f.ty, "expr": f.expr})).collect::<Vec<_>>(),
    });
    std::fs::create_dir_all(&dir)?;
    write_new(&dir.join("mod.rs"), &render(MOD, &vars)?)?;
    write_new(&dir.join("handlers.rs"), &render(HANDLERS, &vars)?)?;
    write_new(&dir.join("json.rs"), &render(JSON, &vars)?)?;
    register(&index, MODS_ANCHOR, &format!("pub mod {module_name};"))?;
    register(&index, ROUTES_ANCHOR, &format!(".nest(\"/{table}\", {module_name}::routes())"))?;
    println!(
        "Permissions `{table}.view_any` and `{table}.view` are declared on the API guard; run `rustcms permission:sync`."
    );
    Ok(ExitCode::SUCCESS)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project() -> (tempfile::TempDir, Project) {
        let dir = tempfile::tempdir().unwrap();
        for (path, contents) in [
            (
                "src/models/blog_post.rs",
                "#[sea_orm(table_name = \"blog_posts\")]\npub struct Model {\n    #[sea_orm(primary_key)]\n    pub id: i64,\n    pub title: String,\n    pub price: Option<Decimal>,\n    pub password_hash: String,\n    pub geo: Point,\n    pub created_at: DateTimeUtc,\n}\n",
            ),
            (
                "src/http/api/mod.rs",
                "// rustcms:api-mods\n\npub fn routes() -> ApiRoutes {\n    ApiRoutes::new(\"/api/v1\")\n        // rustcms:api-routes\n}\n",
            ),
        ] {
            let file = dir.path().join(path);
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(file, contents).unwrap();
        }
        let project = Project { root: dir.path().to_path_buf(), package: "blog".into() };
        (dir, project)
    }

    #[test]
    fn writes_a_module_and_registers_it_once() {
        let (dir, project) = project();
        make_api(&project, "BlogPost").unwrap();
        let root = dir.path().join("src/http/api");
        let index = std::fs::read_to_string(root.join("mod.rs")).unwrap();
        assert!(index.contains("pub mod blog_posts;\n// rustcms:api-mods"), "{index}");
        assert!(
            index.contains(".nest(\"/blog_posts\", blog_posts::routes())\n        // rustcms:api-routes"),
            "{index}"
        );
        let routes = std::fs::read_to_string(root.join("blog_posts/mod.rs")).unwrap();
        assert!(routes.contains("Access::can(\"blog_posts.view_any\"), handlers::list"));
        let handlers = std::fs::read_to_string(root.join("blog_posts/handlers.rs")).unwrap();
        assert!(handlers.contains("Path(id): Path<i64>") && handlers.contains("order_by_desc(blog_post::Column::Id)"));
        let json = std::fs::read_to_string(root.join("blog_posts/json.rs")).unwrap();
        assert!(json.contains("id: String,") && json.contains("id: m.id.to_string(),"), "{json}");
        assert!(json.contains("price: Option<String>,") && json.contains("price: m.price.map(|v| v.to_string()),"));
        assert!(!json.contains("password_hash") && !json.contains("geo"), "secrets and unsupported types left out");
        assert!(make_api(&project, "BlogPost").is_err(), "never overwrites");
    }

    #[test]
    fn unusual_models_give_valid_code() {
        let (dir, project) = project();
        // A multi-line secret alias, a raw field, a renamed key variant, a scaled
        // decimal, an offset time, a table named in a comment first.
        std::fs::write(
            dir.path().join("src/models/item.rs"),
            r##"// The old table_name = "wrong" was renamed.
#[sea_orm(
    table_name = "stock-items"
)]
pub struct Model {
    #[sea_orm(primary_key, enum_name = "Code")]
    pub item_code: u32,
    #[sea_orm(
        column_name = "api_key"
    )]
    pub credential: String,
    pub r#type: String,
    #[sea_orm(column_type = "Decimal(Some((10, 2)))")]
    pub price: Decimal,
    pub seen_at: Option<DateTimeWithTimeZone>,
}
"##,
        )
        .unwrap();
        make_api(&project, "Item").unwrap();
        let root = dir.path().join("src/http/api");
        let index = std::fs::read_to_string(root.join("mod.rs")).unwrap();
        assert!(
            index.contains("pub mod stock_items;") && index.contains(".nest(\"/stock-items\", stock_items::routes())"),
            "{index}"
        );
        let json = std::fs::read_to_string(root.join("stock_items/json.rs")).unwrap();
        assert!(!json.contains("credential"), "a secret behind a multi-line alias stays out: {json}");
        assert!(json.contains("r#type: String,") && json.contains("r#type: m.r#type,"), "{json}");
        assert!(json.contains("price: format!(\"{:.2}\", m.price),"), "{json}");
        assert!(
            json.contains("seen_at: m.seen_at.map(|v| v.naive_utc().format(\"%Y-%m-%dT%H:%M:%S%.fZ\").to_string()),"),
            "{json}"
        );
        let handlers = std::fs::read_to_string(root.join("stock_items/handlers.rs")).unwrap();
        assert!(handlers.contains("Path<u32>") && handlers.contains("Column::Code)"), "{handlers}");
        assert!(handlers.contains("highest key first") && !handlers.contains("CreatedAt"), "{handlers}");
        // The default model sorts by creation time, then key.
        make_api(&project, "BlogPost").unwrap();
        let handlers = std::fs::read_to_string(root.join("blog_posts/handlers.rs")).unwrap();
        assert!(
            handlers.contains(
                ".order_by_desc(blog_post::Column::CreatedAt)\n        .order_by_desc(blog_post::Column::Id)"
            ),
            "{handlers}"
        );
        assert_eq!(module_ident("type"), "type_");
        assert_eq!(module_ident("2024-logs"), "t_2024_logs");
    }

    #[test]
    fn refuses_what_it_cannot_generate() {
        let (dir, project) = project();
        assert!(make_api(&project, "Missing").is_err());
        std::fs::write(
            dir.path().join("src/models/pair.rs"),
            "pub struct Model {\n    #[sea_orm(primary_key)]\n    pub a: i32,\n    #[sea_orm(primary_key)]\n    pub b: i32,\n}\n",
        )
        .unwrap();
        assert!(make_api(&project, "Pair").unwrap_err().to_string().contains("composite"));
        // Without the anchors nothing is written.
        std::fs::write(dir.path().join("src/http/api/mod.rs"), "pub fn routes() {}\n").unwrap();
        assert!(make_api(&project, "BlogPost").is_err());
        assert!(!dir.path().join("src/http/api/blog_posts").exists());
    }
}
