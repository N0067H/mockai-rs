use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, State},
};
use serde::Serialize;

use crate::{app::AppState, error::ApiError, test_data::Model};

#[derive(Serialize)]
pub(crate) struct ModelResponse {
    id: String,
    object: &'static str,
    created: u64,
    owned_by: String,
}

impl From<&Model> for ModelResponse {
    fn from(model: &Model) -> Self {
        Self {
            id: model.id.clone(),
            object: "model",
            created: model.created,
            owned_by: model.owned_by.clone(),
        }
    }
}

#[derive(Serialize)]
pub(crate) struct ModelList {
    object: &'static str,
    data: Vec<ModelResponse>,
}

pub(crate) async fn list(State(state): State<Arc<AppState>>) -> Json<ModelList> {
    Json(ModelList {
        object: "list",
        data: state
            .test_data
            .models
            .iter()
            .map(ModelResponse::from)
            .collect(),
    })
}

pub(crate) async fn get(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<ModelResponse>, ApiError> {
    state
        .test_data
        .models
        .iter()
        .find(|model| model.id == id)
        .map(|model| Json(ModelResponse::from(model)))
        .ok_or_else(|| ApiError::model_not_found(&id))
}
