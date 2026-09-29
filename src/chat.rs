use std::{
    sync::{Arc, atomic::Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use axum::{
    Json,
    extract::{State, rejection::JsonRejection},
    response::{IntoResponse, Response},
};
use serde_json::{Value, json};

use crate::{app::AppState, error::ApiError};

pub(crate) async fn create(
    State(state): State<Arc<AppState>>,
    payload: Result<Json<Value>, JsonRejection>,
) -> Result<Response, ApiError> {
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
    let messages = object
        .get("messages")
        .and_then(Value::as_array)
        .filter(|messages| !messages.is_empty())
        .ok_or_else(|| {
            ApiError::invalid_request("'messages' must be a non-empty array.", Some("messages"))
        })?;
    let mut prompt_tokens = 0;
    for (index, message) in messages.iter().enumerate() {
        let param = format!("messages[{index}]");
        let message = message.as_object().ok_or_else(|| {
            ApiError::invalid_request("Each message must be an object.", Some(&param))
        })?;
        let role = message.get("role").and_then(Value::as_str);
        if !matches!(role, Some("system" | "developer" | "user" | "assistant")) {
            return Err(ApiError::invalid_request(
                "Supported roles are system, developer, user, and assistant.",
                Some(&format!("{param}.role")),
            ));
        }
        for field in message.keys() {
            if !matches!(field.as_str(), "role" | "content" | "name") {
                return Err(ApiError::invalid_request(
                    "This message field is not supported yet.",
                    Some(&format!("{param}.{field}")),
                ));
            }
        }
        if let Some(name) = message.get("name")
            && !name.is_string()
        {
            return Err(ApiError::invalid_request(
                "Message name must be a string.",
                Some(&format!("{param}.name")),
            ));
        }
        let content_param = format!("{param}.content");
        match message.get("content") {
            Some(Value::String(text)) => prompt_tokens += word_count(text),
            Some(Value::Array(parts)) if !parts.is_empty() => {
                for part in parts {
                    if part.get("type").and_then(Value::as_str) != Some("text")
                        || part.as_object().is_none_or(|part| part.len() != 2)
                    {
                        return Err(ApiError::invalid_request(
                            "Only text content parts are supported.",
                            Some(&content_param),
                        ));
                    }
                    let text = part.get("text").and_then(Value::as_str).ok_or_else(|| {
                        ApiError::invalid_request(
                            "Text content parts need a string 'text'.",
                            Some(&content_param),
                        )
                    })?;
                    prompt_tokens += word_count(text);
                }
            }
            _ => {
                return Err(ApiError::invalid_request(
                    "Message content must be text or a non-empty array of text parts.",
                    Some(&content_param),
                ));
            }
        }
    }
    for (field, value) in object {
        if value.is_null()
            && matches!(
                field.as_str(),
                "stream" | "store" | "n" | "temperature" | "top_p" | "response_format"
            )
        {
            continue;
        }
        let valid = match field.as_str() {
            "model" | "messages" => true,
            "stream" => value.is_boolean(),
            "store" => value == &json!(false),
            "stream_options" => {
                value.is_null()
                    || (object.get("stream") == Some(&json!(true))
                        && value.as_object().is_some_and(|options| {
                            options
                                .iter()
                                .all(|(key, value)| key == "include_usage" && value.is_boolean())
                        }))
            }
            "n" => value.as_u64() == Some(1),
            "temperature" => value
                .as_f64()
                .is_some_and(|value| (0.0..=2.0).contains(&value)),
            "top_p" => value
                .as_f64()
                .is_some_and(|value| (0.0..=1.0).contains(&value)),
            "response_format" => value == &json!({"type":"text"}),
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
    let completion_tokens = word_count(&model.reply);
    let sequence = state.next_completion_id.fetch_add(1, Ordering::Relaxed);
    let created = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let response = json!({
        "id": format!("chatcmpl-mock-{sequence}"),
        "object": "chat.completion",
        "created": created,
        "model": model.id,
        "choices": [{"index":0,"message":{"role":"assistant","content":model.reply,"refusal":null},"finish_reason":"stop","logprobs":null}],
        "usage": {"prompt_tokens":prompt_tokens,"completion_tokens":completion_tokens,"total_tokens":prompt_tokens+completion_tokens}
    });
    if object.get("stream") == Some(&json!(true)) {
        let include_usage = object
            .get("stream_options")
            .and_then(|options| options.get("include_usage"))
            .and_then(Value::as_bool)
            .unwrap_or(false);
        Ok(crate::streaming::chat(response, include_usage))
    } else {
        Ok(Json(response).into_response())
    }
}

fn word_count(text: &str) -> usize {
    text.split_whitespace().count()
}

#[cfg(test)]
mod tests {
    use axum::{
        Router,
        body::{Body, to_bytes},
        http::{Request, StatusCode},
    };
    use serde_json::{Value, json};
    use tower::ServiceExt;

    use crate::{app, test_data::TestData};

    async fn send(
        app: Router,
        body: String,
        auth: bool,
        content_type: bool,
    ) -> (StatusCode, Value) {
        let mut request = Request::builder()
            .method("POST")
            .uri("/v1/chat/completions");
        if auth {
            request = request.header("Authorization", "Bearer test-key");
        }
        if content_type {
            request = request.header("Content-Type", "application/json");
        }
        let response = app
            .oneshot(request.body(Body::from(body)).unwrap())
            .await
            .unwrap();
        assert_eq!(response.headers()["content-type"], "application/json");
        let status = response.status();
        let body =
            serde_json::from_slice(&to_bytes(response.into_body(), 4096).await.unwrap()).unwrap();
        (status, body)
    }

    fn router() -> Router {
        app::router("test-key".into(), TestData::load(None).unwrap())
    }

    #[tokio::test]
    async fn fixed_replies_have_unique_ids_and_usage() {
        let data = serde_json::from_value(
            json!({"models":[{"id":"custom","reply":"A fixed reply","embedding":[1]}]}),
        )
        .unwrap();
        let app = app::router("test-key".into(), data);
        let request = json!({"model":"custom","messages":[
            {"role":"developer","content":"Keep it short"},
            {"role":"user","content":[{"type":"text","text":"Hello world"}]}
        ],"stream":false,"store":false,"n":1,"temperature":0,"top_p":1,"response_format":{"type":"text"}}).to_string();
        let (status, first) = send(app.clone(), request.clone(), true, true).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(first["object"], "chat.completion");
        assert_eq!(first["model"], "custom");
        assert!(first["created"].as_u64().unwrap() > 0);
        assert_eq!(
            first["choices"],
            json!([{"index":0,"message":{"role":"assistant","content":"A fixed reply","refusal":null},"finish_reason":"stop","logprobs":null}])
        );
        assert_eq!(
            first["usage"],
            json!({"prompt_tokens":5,"completion_tokens":3,"total_tokens":8})
        );
        let (status, second) = send(app, request, true, true).await;
        assert_eq!(status, StatusCode::OK);
        assert_ne!(first["id"], second["id"]);
        assert!(first["id"].as_str().unwrap().starts_with("chatcmpl-"));
        assert_eq!(first["choices"], second["choices"]);
        assert_eq!(first["usage"], second["usage"]);
    }

    #[tokio::test]
    async fn invalid_input_and_unsupported_features_return_errors() {
        let base = json!({"model":"mock-model","messages":[{"role":"user","content":"Hello"}]});
        let mut cases = vec![
            (json!({"messages":base["messages"]}), "model"),
            (json!({"model":"mock-model"}), "messages"),
            (json!({"model":"mock-model","messages":[]}), "messages"),
            (
                json!({"model":"mock-model","messages":[{"role":"bad","content":"Hi"}]}),
                "messages[0].role",
            ),
            (
                json!({"model":"mock-model","messages":[{"role":"user","content":null}]}),
                "messages[0].content",
            ),
            (
                json!({"model":"mock-model","messages":[{"role":"user","content":[{"type":"image_url","image_url":{"url":"test"}}]}]}),
                "messages[0].content",
            ),
        ];
        for (field, value) in [
            ("stream", json!("bad")),
            ("store", json!(true)),
            ("n", json!(2)),
            ("temperature", json!(3)),
            ("tools", json!([])),
            ("response_format", json!({"type":"json_object"})),
            ("unknown", Value::Null),
        ] {
            let mut request = base.clone();
            request[field] = value;
            cases.push((request, field));
        }
        for (request, param) in cases {
            let (status, body) = send(router(), request.to_string(), true, true).await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{request}");
            assert_eq!(body["error"]["type"], "invalid_request_error");
            assert_eq!(body["error"]["param"], param);
        }
        let mut request = base;
        request["model"] = json!("missing");
        let (status, body) = send(router(), request.to_string(), true, true).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body["error"]["code"], "model_not_found");
    }

    #[tokio::test]
    async fn auth_and_json_rejections_use_error_envelopes() {
        for (body, auth, content_type, expected) in [
            ("{", false, true, StatusCode::UNAUTHORIZED),
            ("{", true, true, StatusCode::BAD_REQUEST),
            ("{}", true, false, StatusCode::UNSUPPORTED_MEDIA_TYPE),
            ("[]", true, true, StatusCode::BAD_REQUEST),
        ] {
            let (status, body) = send(router(), body.into(), auth, content_type).await;
            assert_eq!(status, expected);
            assert!(body["error"]["message"].is_string());
        }
        let (status, body) = send(
            router(),
            json!({"model":"mock-model","messages":[{"role":"user","content":"Hi"}]}).to_string(),
            true,
            true,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            body["choices"][0]["message"]["content"],
            "Hello from mockai-rs!"
        );
    }
}
