use serde_json::{Map, Value as JsonValue};
use servcat_model::FieldMapping;
use tera::{Context, Tera};

#[derive(Debug, thiserror::Error)]
pub enum RenderError {
    #[error("template for target field '{target_field}' is invalid: {source}")]
    Template {
        target_field: String,
        #[source]
        source: tera::Error,
    },
}

/// Values a field-mapping template may reference: `{{ answers.room_number }}`,
/// `{{ requester.email }}`, `{{ instance.id }}`, etc.
pub struct TemplateContext {
    inner: Context,
}

impl TemplateContext {
    pub fn new(
        answers: &JsonValue,
        requester_email: &str,
        requester_display_name: &str,
        instance_id: &str,
        catalog_item_name: &str,
    ) -> Self {
        let mut inner = Context::new();
        inner.insert("answers", answers);
        inner.insert(
            "requester",
            &serde_json::json!({ "email": requester_email, "display_name": requester_display_name }),
        );
        inner.insert(
            "instance",
            &serde_json::json!({ "id": instance_id, "catalog_item_name": catalog_item_name }),
        );
        Self { inner }
    }
}

/// Renders each `target_field -> template` pair in `mapping` against
/// `context`, then expands dot-separated target field names (e.g.
/// `"project.key"`) into nested JSON objects -- Jira/GLPI-style APIs expect
/// `{"project": {"key": "..."}}` rather than a flat `"project.key"` key.
pub fn render(mapping: &FieldMapping, context: &TemplateContext) -> Result<JsonValue, RenderError> {
    let mut root = Map::new();

    for (target_field, template) in mapping.iter() {
        let rendered = Tera::one_off(template, &context.inner, false).map_err(|source| RenderError::Template {
            target_field: target_field.clone(),
            source,
        })?;
        set_dot_path(&mut root, target_field, JsonValue::String(rendered));
    }

    Ok(JsonValue::Object(root))
}

fn set_dot_path(root: &mut Map<String, JsonValue>, path: &str, value: JsonValue) {
    let mut segments = path.split('.').peekable();
    let mut current = root;

    while let Some(segment) = segments.next() {
        if segments.peek().is_none() {
            current.insert(segment.to_string(), value);
            return;
        }

        let entry = current
            .entry(segment.to_string())
            .or_insert_with(|| JsonValue::Object(Map::new()));
        // A mapping that assigns both e.g. "project" and "project.key" would
        // collide; last write wins by resetting to an empty object.
        if !entry.is_object() {
            *entry = JsonValue::Object(Map::new());
        }
        current = entry.as_object_mut().expect("just ensured object");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn renders_and_nests_dot_paths() {
        let mut fields = HashMap::new();
        fields.insert("summary".to_string(), "New request: {{ instance.catalog_item_name }}".to_string());
        fields.insert("project.key".to_string(), "ITHD".to_string());
        fields.insert("issuetype.name".to_string(), "Task".to_string());
        let mapping = FieldMapping(fields);

        let answers = serde_json::json!({ "room": "204" });
        let ctx = TemplateContext::new(&answers, "a@b.com", "A B", "inst-1", "New Laptop");

        let rendered = render(&mapping, &ctx).unwrap();
        assert_eq!(rendered["summary"], "New request: New Laptop");
        assert_eq!(rendered["project"]["key"], "ITHD");
        assert_eq!(rendered["issuetype"]["name"], "Task");
    }
}
