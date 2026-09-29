use std::{collections::HashSet, fs, io, path::Path};

use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TestData {
    pub models: Vec<Model>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Model {
    pub id: String,
    pub reply: String,
    pub embedding: Vec<f32>,
}

impl TestData {
    pub fn load(path: Option<&Path>) -> io::Result<Self> {
        let data = match path {
            Some(path) => fs::read_to_string(path).map_err(|error| {
                io::Error::new(
                    error.kind(),
                    format!("Cannot read test data {}: {error}", path.display()),
                )
            })?,
            None => include_str!("../fixtures/default.json").to_owned(),
        };
        Self::parse(&data).map_err(|error| {
            let source = path
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| "built-in data".into());
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("Invalid test data in {source}: {error}"),
            )
        })
    }

    fn parse(data: &str) -> Result<Self, String> {
        let data: Self = serde_json::from_str(data).map_err(|error| error.to_string())?;
        if data.models.is_empty() {
            return Err("at least one model is required".into());
        }
        let mut ids = HashSet::new();
        for model in &data.models {
            if model.id.is_empty()
                || !model
                    .id
                    .bytes()
                    .all(|byte| byte.is_ascii_graphic() && byte != b'/')
            {
                return Err(
                    "model IDs must be non-empty ASCII text with no spaces or slashes".into(),
                );
            }
            if !ids.insert(&model.id) {
                return Err(format!("duplicate model ID: {}", model.id));
            }
            if model.reply.is_empty() {
                return Err(format!("model {} needs a non-empty reply", model.id));
            }
            if model.embedding.is_empty() || model.embedding.iter().any(|value| !value.is_finite())
            {
                return Err(format!(
                    "model {} needs a non-empty vector of finite numbers",
                    model.id
                ));
            }
        }
        Ok(data)
    }
}

#[cfg(test)]
mod tests {
    use super::TestData;

    #[test]
    fn built_in_data_is_repeatable() {
        let first = TestData::load(None).unwrap();
        let second = TestData::load(None).unwrap();
        assert_eq!(first.models[0].id, "mock-model");
        assert_eq!(first.models[0].reply, second.models[0].reply);
        assert_eq!(first.models[0].embedding, second.models[0].embedding);
    }

    #[test]
    fn custom_data_replaces_defaults() {
        let data = TestData::parse(r#"{"models":[{"id":"test-a","reply":"A","embedding":[1,2]},{"id":"test-b","reply":"B","embedding":[3,4]}]}"#).unwrap();
        assert_eq!(data.models.len(), 2);
        assert_eq!(data.models[0].id, "test-a");
        assert_eq!(data.models[1].reply, "B");
        assert_eq!(data.models[1].embedding, vec![3.0, 4.0]);
    }

    #[test]
    fn loads_a_file_and_reports_missing_files() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/default.json");
        let data = TestData::load(Some(&path)).unwrap();
        assert_eq!(data.models[0].id, "mock-model");
        let missing = path.with_file_name("missing-test-data.json");
        let error = TestData::load(Some(&missing)).err().unwrap();
        assert_eq!(error.kind(), std::io::ErrorKind::NotFound);
        assert!(error.to_string().contains("missing-test-data.json"));
    }

    #[test]
    fn bad_data_is_rejected() {
        for data in [
            "not JSON",
            r#"{"models":[]}"#,
            r#"{"models":[{"id":"test","reply":"A","embedding":[1]},{"id":"test","reply":"B","embedding":[2]}]}"#,
            r#"{"models":[{"id":"bad id","reply":"A","embedding":[1]}]}"#,
            r#"{"models":[{"id":"test","reply":"","embedding":[1]}]}"#,
            r#"{"models":[{"id":"test","reply":"A","embedding":[]}]}"#,
            r#"{"models":[{"id":"test","reply":"A","embedding":[1]}],"typo":true}"#,
        ] {
            assert!(TestData::parse(data).is_err(), "accepted: {data}");
        }
    }
}
