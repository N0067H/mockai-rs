use std::sync::Arc;

use axum::{
    Json,
    extract::{State, rejection::JsonRejection},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::{Value, json};

use crate::{app::AppState, error::ApiError};

pub(crate) async fn create(
    State(state): State<Arc<AppState>>,
    payload: Result<Json<Value>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Json(request) = payload.map_err(|error| {
        ApiError::request_error(
            error.status(),
            "Invalid JSON request body or content type.",
            None,
        )
    })?;
    let object = request
        .as_object()
        .ok_or_else(|| ApiError::invalid_request("Request body must be a JSON object.", None))?;
    let id = object
        .get("model")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
        .ok_or_else(|| {
            ApiError::invalid_request("'model' must be a non-empty string.", Some("model"))
        })?;
    let counts = input_counts(object.get("input"))?;
    for (field, value) in object {
        let valid = match field.as_str() {
            "model" | "input" | "dimensions" => true,
            "encoding_format" => {
                value.is_null() || matches!(value.as_str(), Some("float" | "base64"))
            }
            "user" => value.is_null() || value.is_string(),
            _ => false,
        };
        if !valid {
            return Err(ApiError::invalid_request(
                format!("Invalid or unsupported option: '{field}'."),
                Some(field),
            ));
        }
    }
    let model = state
        .test_data
        .models
        .iter()
        .find(|model| model.id == id)
        .ok_or_else(|| ApiError::model_not_found(id))?;
    let dimensions = match object.get("dimensions").filter(|value| !value.is_null()) {
        None => model.embedding.len(),
        Some(value) => value
            .as_u64()
            .and_then(|value| usize::try_from(value).ok())
            .filter(|value| *value > 0 && *value <= model.embedding.len())
            .ok_or_else(|| {
                ApiError::invalid_request(
                    "'dimensions' must be a positive integer no larger than the test vector.",
                    Some("dimensions"),
                )
            })?,
    };
    let vector = &model.embedding[..dimensions];
    let embedding = if object.get("encoding_format").and_then(Value::as_str) == Some("base64") {
        let bytes: Vec<u8> = vector
            .iter()
            .flat_map(|value| value.to_le_bytes())
            .collect();
        json!(STANDARD.encode(bytes))
    } else {
        json!(vector)
    };
    let data: Vec<Value> = counts
        .iter()
        .enumerate()
        .map(|(index, _)| json!({"object":"embedding","index":index,"embedding":embedding}))
        .collect();
    let tokens: usize = counts.iter().sum();
    Ok(Json(
        json!({"object":"list","data":data,"model":model.id,"usage":{"prompt_tokens":tokens,"total_tokens":tokens}}),
    ))
}

fn input_counts(input: Option<&Value>) -> Result<Vec<usize>, ApiError> {
    let invalid = || {
        ApiError::invalid_request(
            "'input' must be non-empty text, a text array, a token array, or an array of token arrays.",
            Some("input"),
        )
    };
    match input {
        Some(Value::String(text)) if !text.is_empty() => Ok(vec![text.split_whitespace().count()]),
        Some(Value::Array(items)) if !items.is_empty() => {
            if items.iter().all(is_token) {
                if items.len() > 8192 {
                    return Err(invalid());
                }
                return Ok(vec![items.len()]);
            }
            if items.len() > 2048 {
                return Err(invalid());
            }
            if items
                .iter()
                .all(|item| item.as_str().is_some_and(|text| !text.is_empty()))
            {
                return Ok(items
                    .iter()
                    .map(|item| item.as_str().unwrap().split_whitespace().count())
                    .collect());
            }
            if items.iter().all(|item| {
                item.as_array().is_some_and(|tokens| {
                    !tokens.is_empty() && tokens.len() <= 8192 && tokens.iter().all(is_token)
                })
            }) {
                return Ok(items
                    .iter()
                    .map(|item| item.as_array().unwrap().len())
                    .collect());
            }
            Err(invalid())
        }
        _ => Err(invalid()),
    }
}

fn is_token(value: &Value) -> bool {
    value
        .as_u64()
        .is_some_and(|value| u32::try_from(value).is_ok())
}

#[cfg(test)]
mod tests {
    use crate::app;
    use axum::{
        Router,
        body::{Body, to_bytes},
        http::{Request, StatusCode},
    };
    use base64::{Engine, engine::general_purpose::STANDARD};
    use serde_json::{Value, json};
    use tower::ServiceExt;

    fn router() -> Router {
        let data = serde_json::from_value(
            json!({"models":[{"id":"custom","reply":"Hi","embedding":[1.0,-2.0,0.5]}]}),
        )
        .unwrap();
        app::router("test-key".into(), data)
    }

    async fn send(body: String, auth: bool, content_type: bool) -> (StatusCode, Value) {
        let mut request = Request::builder().method("POST").uri("/v1/embeddings");
        if auth {
            request = request.header("Authorization", "Bearer test-key");
        }
        if content_type {
            request = request.header("Content-Type", "application/json");
        }
        let response = router()
            .oneshot(request.body(Body::from(body)).unwrap())
            .await
            .unwrap();
        assert_eq!(response.headers()["content-type"], "application/json");
        let status = response.status();
        let body =
            serde_json::from_slice(&to_bytes(response.into_body(), 8192).await.unwrap()).unwrap();
        (status, body)
    }

    #[tokio::test]
    async fn text_and_token_inputs_return_ordered_fixed_vectors() {
        for (input, count, tokens) in [
            (json!("Hello world"), 1, 2),
            (json!(["Hello world", "Hi"]), 2, 3),
            (json!([10, 20, 30]), 1, 3),
            (json!([[10, 20], [30]]), 2, 3),
        ] {
            let request = json!({"model":"custom","input":input});
            let (status, first) = send(request.to_string(), true, true).await;
            assert_eq!(status, StatusCode::OK);
            assert_eq!(first["object"], "list");
            assert_eq!(first["model"], "custom");
            assert_eq!(first["data"].as_array().unwrap().len(), count);
            for index in 0..count {
                assert_eq!(
                    first["data"][index],
                    json!({"object":"embedding","index":index,"embedding":[1.0,-2.0,0.5]})
                );
            }
            assert_eq!(
                first["usage"],
                json!({"prompt_tokens":tokens,"total_tokens":tokens})
            );
            let (_, second) = send(request.to_string(), true, true).await;
            assert_eq!(first, second);
        }
    }

    #[tokio::test]
    async fn dimensions_and_base64_match_float_bytes() {
        let request = json!({"model":"custom","input":["Hi","Hello"],"dimensions":2,"encoding_format":"base64","user":"test-user"});
        let (status, body) = send(request.to_string(), true, true).await;
        assert_eq!(status, StatusCode::OK);
        let expected: Vec<u8> = [1.0_f32, -2.0]
            .iter()
            .flat_map(|value| value.to_le_bytes())
            .collect();
        for item in body["data"].as_array().unwrap() {
            assert_eq!(
                STANDARD
                    .decode(item["embedding"].as_str().unwrap())
                    .unwrap(),
                expected
            );
        }
        let (status, body) = send(
            json!({"model":"custom","input":"Hi","dimensions":1,"encoding_format":"float"})
                .to_string(),
            true,
            true,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["data"][0]["embedding"], json!([1.0]));
    }

    #[tokio::test]
    async fn bad_input_options_and_auth_return_json_errors() {
        let base = json!({"model":"custom","input":"Hi"});
        let mut cases = vec![
            (json!({"input":"Hi"}), "model"),
            (json!({"model":"custom"}), "input"),
        ];
        for input in [
            json!(""),
            json!([]),
            json!(["Hi", ""]),
            json!(["Hi", 1]),
            json!([-1]),
            json!([1.5]),
            json!([[]]),
            json!([[1], [-2]]),
            json!(true),
            json!(vec!["Hi"; 2049]),
        ] {
            let mut request = base.clone();
            request["input"] = input;
            cases.push((request, "input"));
        }
        for (field, value) in [
            ("dimensions", json!(0)),
            ("dimensions", json!(4)),
            ("dimensions", json!(1.5)),
            ("encoding_format", json!("bad")),
            ("user", json!(1)),
            ("unknown", Value::Null),
        ] {
            let mut request = base.clone();
            request[field] = value;
            cases.push((request, field));
        }
        for (request, param) in cases {
            let (status, body) = send(request.to_string(), true, true).await;
            assert_eq!(status, StatusCode::BAD_REQUEST);
            assert_eq!(body["error"]["param"], param);
            assert_eq!(body["error"]["type"], "invalid_request_error");
        }
        let (status, body) = send(
            json!({"model":"missing","input":"Hi"}).to_string(),
            true,
            true,
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body["error"]["code"], "model_not_found");
        for (body, auth, content_type, expected) in [
            ("{", false, true, StatusCode::UNAUTHORIZED),
            ("{", true, true, StatusCode::BAD_REQUEST),
            ("{}", true, false, StatusCode::UNSUPPORTED_MEDIA_TYPE),
            ("[]", true, true, StatusCode::BAD_REQUEST),
        ] {
            let (status, body) = send(body.into(), auth, content_type).await;
            assert_eq!(status, expected);
            assert!(body["error"]["message"].is_string());
        }
        let response = router()
            .oneshot(
                Request::builder()
                    .uri("/v1/embeddings")
                    .header("Authorization", "Bearer test-key")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
    }
}
