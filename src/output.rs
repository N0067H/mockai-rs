use crate::{
    error::ApiError,
    test_data::{Model, ToolCall, valid_name},
};
use serde_json::{Map, Value, json};

fn bad(message: &str, param: &str) -> ApiError {
    ApiError::invalid_request(message, Some(param))
}

fn fields(object: &Map<String, Value>, allowed: &[&str], param: &str) -> Result<(), ApiError> {
    if object.keys().any(|key| !allowed.contains(&key.as_str())) {
        return Err(bad("Unsupported field.", param));
    }
    Ok(())
}

pub fn text(model: &Model, format: Option<&Value>, chat: bool) -> Result<String, ApiError> {
    let param = if chat {
        "response_format"
    } else {
        "text.format"
    };
    let format = if chat {
        format
    } else {
        match format.filter(|value| !value.is_null()) {
            None => None,
            Some(value) => {
                let object = value
                    .as_object()
                    .ok_or_else(|| bad("'text' must be an object.", "text"))?;
                fields(object, &["format"], "text")?;
                object.get("format")
            }
        }
    };
    let Some(format) = format.filter(|value| !value.is_null()) else {
        return Ok(model.reply.clone());
    };
    let object = format
        .as_object()
        .ok_or_else(|| bad("Output format must be an object.", param))?;
    match object.get("type").and_then(Value::as_str) {
        Some("text") => {
            fields(object, &["type"], param)?;
            Ok(model.reply.clone())
        }
        Some("json_object") => {
            fields(object, &["type"], param)?;
            Ok(model
                .json_reply
                .clone()
                .unwrap_or_else(|| json!({"message":model.reply}))
                .to_string())
        }
        Some("json_schema") => {
            let schema_config = if chat {
                fields(object, &["type", "json_schema"], param)?;
                object
                    .get("json_schema")
                    .and_then(Value::as_object)
                    .ok_or_else(|| bad("'json_schema' must be an object.", param))?
            } else {
                object
            };
            let allowed = if chat {
                vec!["name", "schema", "strict", "description"]
            } else {
                vec!["type", "name", "schema", "strict", "description"]
            };
            fields(schema_config, &allowed, param)?;
            if !schema_config
                .get("name")
                .and_then(Value::as_str)
                .is_some_and(valid_name)
            {
                return Err(bad("Schema needs a valid name.", param));
            }
            if schema_config
                .get("strict")
                .is_some_and(|value| !value.is_null() && !value.is_boolean())
                || schema_config
                    .get("description")
                    .is_some_and(|value| !value.is_string())
            {
                return Err(bad("Invalid schema options.", param));
            }
            let schema = schema_config
                .get("schema")
                .filter(|value| value.is_object())
                .ok_or_else(|| bad("Schema must be an object.", param))?;
            let reply = model
                .json_reply
                .clone()
                .unwrap_or_else(|| json!({"message":model.reply}));
            validate_schema(schema, &reply, param)?;
            Ok(reply.to_string())
        }
        _ => Err(bad(
            "Supported formats are text, json_object, and json_schema.",
            param,
        )),
    }
}

fn validate_schema(schema: &Value, instance: &Value, param: &str) -> Result<(), ApiError> {
    let validator = jsonschema::validator_for(schema).map_err(|_| bad("Invalid schema or unresolved reference. External schema files and URLs are not supported.", param))?;
    if !validator.is_valid(instance) {
        return Err(bad(
            "Fixed test data does not match the requested schema.",
            param,
        ));
    }
    Ok(())
}

pub fn calls(
    model: &Model,
    request: &Map<String, Value>,
    chat: bool,
    has_result: bool,
) -> Result<Vec<ToolCall>, ApiError> {
    let mut definitions = Vec::new();
    if let Some(tools) = request.get("tools").filter(|value| !value.is_null()) {
        for tool in tools
            .as_array()
            .ok_or_else(|| bad("'tools' must be an array.", "tools"))?
        {
            let tool = tool
                .as_object()
                .ok_or_else(|| bad("Each tool must be an object.", "tools"))?;
            if tool.get("type").and_then(Value::as_str) != Some("function") {
                return Err(bad("Only function tools are supported.", "tools"));
            }
            let function = if chat {
                fields(tool, &["type", "function"], "tools")?;
                tool.get("function")
                    .and_then(Value::as_object)
                    .ok_or_else(|| bad("Tool needs a function object.", "tools"))?
            } else {
                tool
            };
            fields(
                function,
                if chat {
                    &["name", "description", "parameters", "strict"]
                } else {
                    &["type", "name", "description", "parameters", "strict"]
                },
                "tools",
            )?;
            let name = function
                .get("name")
                .and_then(Value::as_str)
                .filter(|name| valid_name(name))
                .ok_or_else(|| bad("Tool needs a valid function name.", "tools"))?;
            if definitions.iter().any(|(previous, _)| *previous == name) {
                return Err(bad("Tool names must be unique.", "tools"));
            }
            if function
                .get("description")
                .is_some_and(|value| !value.is_string())
                || function
                    .get("strict")
                    .is_some_and(|value| !value.is_null() && !value.is_boolean())
            {
                return Err(bad("Invalid function options.", "tools"));
            }
            let parameters = function.get("parameters").filter(|value| !value.is_null());
            if let Some(schema) = parameters {
                if !schema.is_object() {
                    return Err(bad("Function parameters must be a schema object.", "tools"));
                }
                jsonschema::validator_for(schema).map_err(|_| {
                    bad("Invalid function schema or unresolved reference.", "tools")
                })?;
            }
            definitions.push((name, parameters));
        }
    }
    if request
        .get("parallel_tool_calls")
        .is_some_and(|value| !value.is_null() && !value.is_boolean())
    {
        return Err(bad(
            "'parallel_tool_calls' must be a boolean.",
            "parallel_tool_calls",
        ));
    }
    let choice = request.get("tool_choice").filter(|value| !value.is_null());
    let mut named = None;
    let mode = match choice {
        None => "auto",
        Some(Value::String(mode)) if matches!(mode.as_str(), "auto" | "none" | "required") => {
            mode.as_str()
        }
        Some(Value::Object(choice)) => {
            if choice.get("type").and_then(Value::as_str) != Some("function") {
                return Err(bad("Invalid tool choice.", "tool_choice"));
            }
            let name = if chat {
                fields(choice, &["type", "function"], "tool_choice")?;
                let function = choice
                    .get("function")
                    .and_then(Value::as_object)
                    .ok_or_else(|| bad("Invalid function choice.", "tool_choice"))?;
                fields(function, &["name"], "tool_choice")?;
                function.get("name")
            } else {
                fields(choice, &["type", "name"], "tool_choice")?;
                choice.get("name")
            };
            named = Some(
                name.and_then(Value::as_str)
                    .ok_or_else(|| bad("Tool choice needs a function name.", "tool_choice"))?,
            );
            "required"
        }
        _ => return Err(bad("Invalid tool choice.", "tool_choice")),
    };
    if let Some(name) = named
        && !definitions.iter().any(|(defined, _)| *defined == name)
    {
        return Err(bad("Chosen function is not in tools.", "tool_choice"));
    }
    if mode == "none" || (mode == "auto" && has_result) {
        return Ok(Vec::new());
    }
    let mut calls = Vec::new();
    for call in &model.tool_calls {
        if named.is_some_and(|name| name != call.name) {
            continue;
        }
        if let Some((_, schema)) = definitions.iter().find(|(name, _)| *name == call.name) {
            if let Some(schema) = schema {
                validate_schema(schema, &call.arguments, "tools")?;
            }
            calls.push(call.clone());
        }
    }
    if mode == "required" && calls.is_empty() {
        return Err(bad(
            "No fixed tool call matches the requested tools.",
            "tool_choice",
        ));
    }
    if request.get("parallel_tool_calls") == Some(&json!(false)) {
        calls.truncate(1);
    }
    Ok(calls)
}

pub fn history_calls(value: &Value, param: &str) -> Result<(), ApiError> {
    let calls = value
        .as_array()
        .filter(|calls| !calls.is_empty())
        .ok_or_else(|| bad("Tool calls must be a non-empty array.", param))?;
    for call in calls {
        if !call
            .get("id")
            .and_then(Value::as_str)
            .is_some_and(|id| !id.is_empty())
            || call.get("type").and_then(Value::as_str) != Some("function")
        {
            return Err(bad("Invalid tool call.", param));
        }
        history_function(
            &call["function"]["name"],
            &call["function"]["arguments"],
            param,
        )?;
    }
    Ok(())
}

pub fn history_function(name: &Value, arguments: &Value, param: &str) -> Result<(), ApiError> {
    if !name.as_str().is_some_and(valid_name)
        || !arguments.as_str().is_some_and(|arguments| {
            serde_json::from_str::<Value>(arguments).is_ok_and(|value| value.is_object())
        })
    {
        return Err(bad(
            "Function calls need a valid name and JSON object arguments as a string.",
            param,
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::{app, test_data::TestData};
    use axum::{
        body::{Body, to_bytes},
        http::{Request, StatusCode},
    };
    use serde_json::{Value, json};
    use tower::ServiceExt;

    fn data() -> TestData {
        serde_json::from_value(json!({"models":[{"id":"test","reply":"Done!","embedding":[1],"json_reply":{"city":"서울","ok":true},"tool_calls":[{"name":"weather","arguments":{"city":"서울"}},{"name":"weather","arguments":{"city":"Paris"}}]}]})).unwrap()
    }

    fn base(chat: bool) -> Value {
        if chat {
            json!({"model":"test","messages":[{"role":"user","content":"Weather?"}]})
        } else {
            json!({"model":"test","input":"Weather?"})
        }
    }

    fn tools(chat: bool) -> Value {
        let function = json!({"name":"weather","description":"Get weather","strict":true,"parameters":{"type":"object","properties":{"city":{"type":"string"}},"required":["city"],"additionalProperties":false}});
        if chat {
            json!([{"type":"function","function":function}])
        } else {
            let mut function = function;
            function["type"] = json!("function");
            json!([function])
        }
    }

    async fn send(chat: bool, payload: Value) -> (StatusCode, String, String) {
        let path = if chat {
            "/v1/chat/completions"
        } else {
            "/v1/responses"
        };
        let response = app::router("key".into(), data())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(path)
                    .header("Authorization", "Bearer key")
                    .header("Content-Type", "application/json")
                    .body(Body::from(payload.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status();
        let content_type = response.headers()["content-type"]
            .to_str()
            .unwrap()
            .to_owned();
        let body = String::from_utf8(
            to_bytes(response.into_body(), 65536)
                .await
                .unwrap()
                .to_vec(),
        )
        .unwrap();
        (status, content_type, body)
    }

    fn events(body: &str) -> Vec<Value> {
        body.split("\n\n")
            .filter_map(|frame| {
                frame
                    .lines()
                    .find_map(|line| line.strip_prefix("data:").map(str::trim))
            })
            .filter(|data| *data != "[DONE]")
            .map(|data| serde_json::from_str(data).unwrap())
            .collect()
    }

    #[tokio::test]
    async fn tool_choices_and_result_round_trip() {
        for chat in [true, false] {
            let mut payload = base(chat);
            payload["tools"] = tools(chat);
            payload["tool_choice"] = json!("required");
            let (status, _, body) = send(chat, payload.clone()).await;
            assert_eq!(status, StatusCode::OK);
            let response: Value = serde_json::from_str(&body).unwrap();
            let calls = if chat {
                &response["choices"][0]["message"]["tool_calls"]
            } else {
                &response["output"]
            };
            assert_eq!(calls.as_array().unwrap().len(), 2);
            assert_ne!(calls[0]["id"], calls[1]["id"]);
            let arguments = if chat {
                &calls[0]["function"]["arguments"]
            } else {
                &calls[0]["arguments"]
            };
            assert_eq!(
                serde_json::from_str::<Value>(arguments.as_str().unwrap()).unwrap(),
                json!({"city":"서울"})
            );
            if chat {
                assert_eq!(response["choices"][0]["finish_reason"], "tool_calls");
                assert!(response["choices"][0]["message"]["content"].is_null());
            }
            payload["parallel_tool_calls"] = json!(false);
            payload["tool_choice"] = if chat {
                json!({"type":"function","function":{"name":"weather"}})
            } else {
                json!({"type":"function","name":"weather"})
            };
            let (status, _, body) = send(chat, payload.clone()).await;
            assert_eq!(status, StatusCode::OK);
            let response: Value = serde_json::from_str(&body).unwrap();
            let calls = if chat {
                &response["choices"][0]["message"]["tool_calls"]
            } else {
                &response["output"]
            };
            assert_eq!(calls.as_array().unwrap().len(), 1);
            payload["tool_choice"] = json!("auto");
            if chat {
                payload["messages"]
                    .as_array_mut()
                    .unwrap()
                    .push(response["choices"][0]["message"].clone());
                payload["messages"]
                    .as_array_mut()
                    .unwrap()
                    .push(json!({"role":"tool","tool_call_id":calls[0]["id"],"content":"Sunny"}));
            } else {
                payload["input"] = json!([calls[0],{"type":"function_call_output","call_id":calls[0]["call_id"],"output":"Sunny"}]);
            }
            let (status, _, body) = send(chat, payload.clone()).await;
            assert_eq!(status, StatusCode::OK);
            let response: Value = serde_json::from_str(&body).unwrap();
            let text = if chat {
                &response["choices"][0]["message"]["content"]
            } else {
                &response["output"][0]["content"][0]["text"]
            };
            assert_eq!(text, "Done!");
            payload = base(chat);
            payload["tools"] = tools(chat);
            payload["tool_choice"] = json!("none");
            let (status, _, body) = send(chat, payload).await;
            assert_eq!(status, StatusCode::OK);
            let response: Value = serde_json::from_str(&body).unwrap();
            if chat {
                assert!(
                    response["choices"][0]["message"]
                        .get("tool_calls")
                        .is_none()
                );
            } else {
                assert_eq!(response["output"][0]["type"], "message");
            }
        }
    }

    #[tokio::test]
    async fn tool_streams_rebuild_arguments_and_finish() {
        for chat in [true, false] {
            let mut payload = base(chat);
            payload["tools"] = tools(chat);
            payload["stream"] = json!(true);
            let (status, content_type, body) = send(chat, payload).await;
            assert_eq!(status, StatusCode::OK);
            assert_eq!(content_type, "text/event-stream");
            let events = events(&body);
            for index in 0..2 {
                let arguments: String = events
                    .iter()
                    .filter_map(|event| {
                        if chat {
                            let call = &event["choices"][0]["delta"]["tool_calls"][0];
                            if call["index"] == index {
                                call["function"]["arguments"].as_str()
                            } else {
                                None
                            }
                        } else if event["type"] == "response.function_call_arguments.delta"
                            && event["output_index"] == index
                        {
                            event["delta"].as_str()
                        } else {
                            None
                        }
                    })
                    .collect();
                let expected = if index == 0 { "서울" } else { "Paris" };
                assert_eq!(
                    serde_json::from_str::<Value>(&arguments).unwrap(),
                    json!({"city":expected})
                );
            }
            if chat {
                assert!(body.ends_with("data: [DONE]\n\n") || body.ends_with("data:[DONE]\n\n"));
                assert_eq!(
                    events.last().unwrap()["choices"][0]["finish_reason"],
                    "tool_calls"
                );
            } else {
                for (index, event) in events.iter().enumerate() {
                    assert_eq!(event["sequence_number"], index);
                }
                assert_eq!(events.last().unwrap()["type"], "response.completed");
                assert_eq!(
                    events.last().unwrap()["response"]["output"]
                        .as_array()
                        .unwrap()
                        .len(),
                    2
                );
                assert_eq!(
                    events
                        .iter()
                        .filter(|event| event["type"] == "response.function_call_arguments.done")
                        .count(),
                    2
                );
            }
        }
    }

    fn format(chat: bool, schema: bool) -> Value {
        if !schema {
            return json!({"type":"json_object"});
        }
        let config = json!({"name":"answer","strict":true,"schema":{"type":"object","properties":{"city":{"type":"string"},"ok":{"type":"boolean"}},"required":["city","ok"],"additionalProperties":false}});
        if chat {
            json!({"type":"json_schema","json_schema":config})
        } else {
            let mut config = config;
            config["type"] = json!("json_schema");
            config
        }
    }

    #[tokio::test]
    async fn json_output_and_schema_work_in_plain_and_stream_replies() {
        for chat in [true, false] {
            for schema in [false, true] {
                for stream in [false, true] {
                    let mut payload = base(chat);
                    let format = format(chat, schema);
                    if chat {
                        payload["response_format"] = format;
                    } else {
                        payload["text"] = json!({"format":format});
                    }
                    payload["stream"] = json!(stream);
                    let (status, _, body) = send(chat, payload).await;
                    assert_eq!(status, StatusCode::OK, "{body}");
                    let text = if stream {
                        events(&body)
                            .iter()
                            .filter_map(|event| {
                                if chat {
                                    event["choices"][0]["delta"]["content"].as_str()
                                } else if event["type"] == "response.output_text.delta" {
                                    event["delta"].as_str()
                                } else {
                                    None
                                }
                            })
                            .collect::<String>()
                    } else {
                        let response: Value = serde_json::from_str(&body).unwrap();
                        if chat {
                            response["choices"][0]["message"]["content"]
                                .as_str()
                                .unwrap()
                                .to_owned()
                        } else {
                            response["output"][0]["content"][0]["text"]
                                .as_str()
                                .unwrap()
                                .to_owned()
                        }
                    };
                    assert_eq!(
                        serde_json::from_str::<Value>(&text).unwrap(),
                        json!({"city":"서울","ok":true})
                    );
                }
            }
        }
    }

    #[tokio::test]
    async fn schema_mismatches_and_bad_tool_choices_are_errors() {
        for chat in [true, false] {
            for choice in [
                json!("required"),
                json!("bad"),
                if chat {
                    json!({"type":"function","function":{"name":"missing"}})
                } else {
                    json!({"type":"function","name":"missing"})
                },
            ] {
                let mut payload = base(chat);
                payload["tools"] = json!([]);
                payload["tool_choice"] = choice;
                let (status, _, body) = send(chat, payload).await;
                assert_eq!(status, StatusCode::BAD_REQUEST);
                let error: Value = serde_json::from_str(&body).unwrap();
                assert_eq!(error["error"]["param"], "tool_choice");
            }
            for schema in [
                json!({"type":"object","required":["missing"]}),
                json!({"type":"invalid"}),
                json!({"$ref":"https://example.com/schema.json"}),
            ] {
                let mut payload = base(chat);
                let mut format = format(chat, true);
                if chat {
                    format["json_schema"]["schema"] = schema;
                    payload["response_format"] = format;
                } else {
                    format["schema"] = schema;
                    payload["text"] = json!({"format":format});
                }
                payload["stream"] = json!(true);
                let (status, content_type, _) = send(chat, payload).await;
                assert_eq!(status, StatusCode::BAD_REQUEST);
                assert_eq!(content_type, "application/json");
            }
        }
    }
}
