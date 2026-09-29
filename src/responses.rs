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
    let mut input_tokens = count_input(object.get("input"))?;
    if let Some(instructions) = object.get("instructions").filter(|value| !value.is_null()) {
        input_tokens += instructions
            .as_str()
            .ok_or_else(|| {
                ApiError::invalid_request("'instructions' must be a string.", Some("instructions"))
            })?
            .split_whitespace()
            .count();
    }
    for (field, value) in object {
        if value.is_null()
            && matches!(
                field.as_str(),
                "instructions"
                    | "stream"
                    | "store"
                    | "background"
                    | "temperature"
                    | "top_p"
                    | "text"
                    | "previous_response_id"
            )
        {
            continue;
        }
        let valid = match field.as_str() {
            "model" | "input" | "instructions" => true,
            "stream" => value.is_boolean(),
            "store" | "background" => value == &json!(false),
            "temperature" => value
                .as_f64()
                .is_some_and(|value| (0.0..=2.0).contains(&value)),
            "top_p" => value
                .as_f64()
                .is_some_and(|value| (0.0..=1.0).contains(&value)),
            "text" | "tools" | "tool_choice" | "parallel_tool_calls" => true,
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
    let has_result = object
        .get("input")
        .and_then(Value::as_array)
        .and_then(|items| items.last())
        .is_some_and(|item| item["type"] == "function_call_output");
    let calls = crate::output::calls(model, object, false, has_result)?;
    let reply = crate::output::text(model, object.get("text"), false)?;
    let output_tokens = if calls.is_empty() {
        reply.split_whitespace().count()
    } else {
        calls
            .iter()
            .map(|call| call.arguments.to_string().split_whitespace().count())
            .sum()
    };
    let sequence = state.next_response_id.fetch_add(1, Ordering::Relaxed);
    let created = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let mut response = json!({
        "id":format!("resp_mock_{sequence}"), "object":"response", "created_at":created,
        "status":"completed", "error":null, "incomplete_details":null,
        "model":model.id, "instructions":object.get("instructions").unwrap_or(&Value::Null),
        "output":[{"id":format!("msg_mock_{sequence}"),"type":"message","role":"assistant","status":"completed",
            "content":[{"type":"output_text","text":reply,"annotations":[],"logprobs":[]}]}],
        "usage":{"input_tokens":input_tokens,"input_tokens_details":{"cached_tokens":0},
            "output_tokens":output_tokens,"output_tokens_details":{"reasoning_tokens":0},"total_tokens":input_tokens+output_tokens},
        "store":false, "background":false, "previous_response_id":null,
        "tools":[], "tool_choice":"auto", "parallel_tool_calls":true,
        "text":{"format":{"type":"text"}}, "metadata":{},
        "temperature":object.get("temperature").and_then(Value::as_f64).unwrap_or(1.0),
        "top_p":object.get("top_p").and_then(Value::as_f64).unwrap_or(1.0),
        "max_output_tokens":null, "reasoning":{"effort":null,"summary":null}, "truncation":"disabled"
    });
    response["tools"] = object
        .get("tools")
        .filter(|value| !value.is_null())
        .cloned()
        .unwrap_or_else(|| json!([]));
    response["tool_choice"] = object
        .get("tool_choice")
        .filter(|value| !value.is_null())
        .cloned()
        .unwrap_or_else(|| {
            if response["tools"].as_array().is_some_and(Vec::is_empty) {
                json!("none")
            } else {
                json!("auto")
            }
        });
    response["parallel_tool_calls"] = object
        .get("parallel_tool_calls")
        .filter(|value| !value.is_null())
        .cloned()
        .unwrap_or(json!(true));
    response["text"] = object
        .get("text")
        .filter(|value| !value.is_null())
        .cloned()
        .unwrap_or_else(|| json!({"format":{"type":"text"}}));
    if !calls.is_empty() {
        response["output"] = json!(calls.iter().enumerate().map(|(index,call)| json!({"id":format!("fc_mock_{sequence}_{index}"),"call_id":format!("call_resp_{sequence}_{index}"),"type":"function_call","name":call.name,"arguments":call.arguments.to_string(),"status":"completed"})).collect::<Vec<_>>());
    }
    if object.get("stream") == Some(&json!(true)) {
        Ok(crate::streaming::responses(response))
    } else {
        Ok(Json(response).into_response())
    }
}

fn count_input(input: Option<&Value>) -> Result<usize, ApiError> {
    match input {
        Some(Value::String(text)) => Ok(text.split_whitespace().count()),
        Some(Value::Array(items)) if !items.is_empty() => {
            let mut count = 0;
            for (index, item) in items.iter().enumerate() {
                let param = format!("input[{index}]");
                let item = item.as_object().ok_or_else(|| {
                    ApiError::invalid_request("Input items must be messages.", Some(&param))
                })?;
                if matches!(
                    item.get("type").and_then(Value::as_str),
                    Some("function_call" | "function_call_output")
                ) {
                    let is_call = item
                        .get("type")
                        .is_some_and(|value| value == "function_call");
                    for (field, value) in item {
                        let valid = match field.as_str() {
                            "type" | "call_id" => true,
                            "name" | "arguments" if is_call => true,
                            "output" if !is_call => true,
                            "id" => value.as_str().is_some_and(|id| !id.is_empty()),
                            "status" => value == "completed",
                            _ => false,
                        };
                        if !valid {
                            return Err(ApiError::invalid_request(
                                "Invalid function item field.",
                                Some(&format!("{param}.{field}")),
                            ));
                        }
                    }
                    if !item
                        .get("call_id")
                        .and_then(Value::as_str)
                        .is_some_and(|id| !id.is_empty())
                    {
                        return Err(ApiError::invalid_request(
                            "Function items need a call_id.",
                            Some(&param),
                        ));
                    }
                    if item
                        .get("type")
                        .is_some_and(|value| value == "function_call")
                    {
                        crate::output::history_function(
                            item.get("name").unwrap_or(&Value::Null),
                            item.get("arguments").unwrap_or(&Value::Null),
                            &param,
                        )?;
                        count += item["arguments"]
                            .as_str()
                            .unwrap()
                            .split_whitespace()
                            .count();
                    } else {
                        count += item
                            .get("output")
                            .and_then(Value::as_str)
                            .ok_or_else(|| {
                                ApiError::invalid_request(
                                    "Function output must be a string.",
                                    Some(&param),
                                )
                            })?
                            .split_whitespace()
                            .count();
                    }
                    continue;
                }
                let role = item.get("role").and_then(Value::as_str);
                if !matches!(role, Some("system" | "developer" | "user" | "assistant")) {
                    return Err(ApiError::invalid_request(
                        "Supported roles are system, developer, user, and assistant.",
                        Some(&format!("{param}.role")),
                    ));
                }
                for (field, value) in item {
                    let valid = match field.as_str() {
                        "role" | "content" => true,
                        "type" => value == "message",
                        "id" => value.is_string(),
                        "status" => value == "completed",
                        _ => false,
                    };
                    if !valid {
                        return Err(ApiError::invalid_request(
                            "Invalid or unsupported input message field.",
                            Some(&format!("{param}.{field}")),
                        ));
                    }
                }
                let content_param = format!("{param}.content");
                match item.get("content") {
                    Some(Value::String(text)) => count += text.split_whitespace().count(),
                    Some(Value::Array(parts)) if !parts.is_empty() => {
                        for (part_index, part) in parts.iter().enumerate() {
                            let part_param = format!("{content_param}[{part_index}]");
                            let part = part.as_object().ok_or_else(|| {
                                ApiError::invalid_request(
                                    "Content parts must be text objects.",
                                    Some(&part_param),
                                )
                            })?;
                            let output = role == Some("assistant")
                                && part.get("type").is_some_and(|value| value == "output_text");
                            if !output
                                && !part.get("type").is_some_and(|value| value == "input_text")
                            {
                                return Err(ApiError::invalid_request(
                                    "Only text content parts are supported.",
                                    Some(&format!("{part_param}.type")),
                                ));
                            }
                            for (field, value) in part {
                                let valid = match field.as_str() {
                                    "type" | "text" => true,
                                    "annotations" | "logprobs" if output => {
                                        value.as_array().is_some_and(Vec::is_empty)
                                    }
                                    _ => false,
                                };
                                if !valid {
                                    return Err(ApiError::invalid_request(
                                        "Unsupported text part field.",
                                        Some(&format!("{part_param}.{field}")),
                                    ));
                                }
                            }
                            let text =
                                part.get("text").and_then(Value::as_str).ok_or_else(|| {
                                    ApiError::invalid_request(
                                        "Text parts need a string 'text'.",
                                        Some(&format!("{part_param}.text")),
                                    )
                                })?;
                            count += text.split_whitespace().count();
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
            Ok(count)
        }
        _ => Err(ApiError::invalid_request(
            "'input' must be a string or a non-empty array of messages.",
            Some("input"),
        )),
    }
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

    fn router() -> Router {
        app::router("test-key".into(), TestData::load(None).unwrap())
    }

    async fn send(
        app: Router,
        body: String,
        auth: bool,
        content_type: bool,
    ) -> (StatusCode, Value) {
        let mut request = Request::builder().method("POST").uri("/v1/responses");
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
            serde_json::from_slice(&to_bytes(response.into_body(), 8192).await.unwrap()).unwrap();
        (status, body)
    }

    #[tokio::test]
    async fn string_input_returns_response_output_and_usage() {
        let data = serde_json::from_value(
            json!({"models":[{"id":"custom","reply":"A fixed reply","embedding":[1]}]}),
        )
        .unwrap();
        let app = app::router("test-key".into(), data);
        let request = json!({"model":"custom","input":"Hello world","instructions":"Keep it short","store":false,"temperature":0.5,"text":{"format":{"type":"text"}}});
        let (status, first) = send(app.clone(), request.to_string(), true, true).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(first["object"], "response");
        assert_eq!(first["model"], "custom");
        assert_eq!(first["status"], "completed");
        assert!(first["created_at"].as_u64().unwrap() > 0);
        assert!(first["error"].is_null());
        assert_eq!(first["instructions"], "Keep it short");
        assert_eq!(first["temperature"], 0.5);
        assert_eq!(first["store"], false);
        assert_eq!(first["output"][0]["type"], "message");
        assert_eq!(first["output"][0]["role"], "assistant");
        assert_eq!(
            first["output"][0]["content"],
            json!([{"type":"output_text","text":"A fixed reply","annotations":[],"logprobs":[]}])
        );
        assert_eq!(
            first["usage"],
            json!({"input_tokens":5,"input_tokens_details":{"cached_tokens":0},"output_tokens":3,"output_tokens_details":{"reasoning_tokens":0},"total_tokens":8})
        );
        assert!(first.get("choices").is_none());
        let (status, second) = send(app, request.to_string(), true, true).await;
        assert_eq!(status, StatusCode::OK);
        assert_ne!(first["id"], second["id"]);
        assert_ne!(first["output"][0]["id"], second["output"][0]["id"]);
        assert_eq!(
            first["output"][0]["content"],
            second["output"][0]["content"]
        );
    }

    #[tokio::test]
    async fn message_input_accepts_text_parts_and_previous_output() {
        let app = router();
        let request = json!({"model":"mock-model","input":[
            {"role":"developer","content":"Be brief"},
            {"type":"message","role":"user","content":[{"type":"input_text","text":"Hello world"}]}
        ]});
        let (status, first) = send(app.clone(), request.to_string(), true, true).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(first["usage"]["input_tokens"], 4);
        let mut history = request["input"].as_array().unwrap().clone();
        history.extend(first["output"].as_array().unwrap().clone());
        history.push(json!({"role":"user","content":"Again"}));
        let (status, second) = send(
            app,
            json!({"model":"mock-model","input":history}).to_string(),
            true,
            true,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(second["usage"]["input_tokens"], 8);
    }

    #[tokio::test]
    async fn invalid_requests_and_unsupported_options_return_json_errors() {
        let base = json!({"model":"mock-model","input":"Hello"});
        let mut cases = vec![
            (json!({"input":"Hi"}), "model"),
            (json!({"model":"mock-model"}), "input"),
            (json!({"model":"mock-model","input":[]}), "input"),
            (
                json!({"model":"mock-model","input":[{"role":"tool","content":"Hi"}]}),
                "input[0].role",
            ),
            (
                json!({"model":"mock-model","input":[{"role":"user","content":[{"type":"input_image","image_url":"test"}]}]}),
                "input[0].content[0].type",
            ),
        ];
        for (field, value) in [
            ("instructions", json!(5)),
            ("store", json!(true)),
            ("stream", json!("bad")),
            ("background", json!(true)),
            ("previous_response_id", json!("resp_123")),
            ("tools", json!({})),
            ("text", json!({"format":{"type":"bad"}})),
            ("temperature", json!(3)),
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
            assert_eq!(
                body["error"]["param"],
                if param == "text" {
                    "text.format"
                } else {
                    param
                }
            );
        }
        let (status, body) = send(
            router(),
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
            let (status, body) = send(router(), body.into(), auth, content_type).await;
            assert_eq!(status, expected);
            assert!(body["error"]["message"].is_string());
        }
        let response = router()
            .oneshot(
                Request::builder()
                    .uri("/v1/responses")
                    .header("Authorization", "Bearer test-key")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
    }
}
