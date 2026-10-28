//! Integrations: an API token and environment variables kept encrypted. The
//! panel never shows them: the token is replaced or cleared, the variables
//! are listed by name and each kept, replaced or removed.

use rustcms::admin::*;

use crate::models::integration;

pub struct IntegrationResource;

impl Resource for IntegrationResource {
    type Entity = integration::Entity;
    const SLUG: &'static str = "integrations";

    fn meta() -> ResourceMeta {
        ResourceMeta::new("Integration").title("name").icon("key").group("Settings")
    }

    // #region fields
    fn fields() -> Vec<Field> {
        vec![
            Field::column(integration::Column::Name).required().searchable().sortable().max_length(60),
            Field::column(integration::Column::ApiToken).encrypted().label("API token"),
            Field::column(integration::Column::Env).encrypted_map().max_items(20).label("Environment"),
        ]
    }
    // #endregion fields
}
