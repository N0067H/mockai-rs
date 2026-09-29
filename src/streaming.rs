use std::convert::Infallible;

use axum::response::{IntoResponse, Response, Sse, sse::Event};
use futures_util::stream;
use serde_json::{Value, json};

fn send(events: Vec<Event>) -> Response {
    Sse::new(stream::iter(events.into_iter().map(Ok::<_, Infallible>))).into_response()
}

fn chunks(text: &str) -> Vec<String> {
    text.chars()
        .collect::<Vec<_>>()
        .chunks(16)
        .map(|chunk| chunk.iter().collect())
        .collect()
}

pub(crate) fn chat(response: Value, include_usage: bool) -> Response {
    let chunk = |delta: Value, finish: Value| {
        json!({
            "id":response["id"],"object":"chat.completion.chunk","created":response["created"],"model":response["model"],
            "choices":[{"index":0,"delta":delta,"finish_reason":finish,"logprobs":null}],"usage":null
        })
    };
    let mut events = vec![
        Event::default()
            .data(chunk(json!({"role":"assistant","content":""}), Value::Null).to_string()),
    ];
    for text in chunks(
        response["choices"][0]["message"]["content"]
            .as_str()
            .unwrap(),
    ) {
        events.push(Event::default().data(chunk(json!({"content":text}), Value::Null).to_string()));
    }
    events.push(Event::default().data(chunk(json!({}), json!("stop")).to_string()));
    if include_usage {
        let mut usage = chunk(json!({}), Value::Null);
        usage["choices"] = json!([]);
        usage["usage"] = response["usage"].clone();
        events.push(Event::default().data(usage.to_string()));
    }
    events.push(Event::default().data("[DONE]"));
    send(events)
}

pub(crate) fn responses(response: Value) -> Response {
    let item = response["output"][0].clone();
    let part = item["content"][0].clone();
    let mut pending = response.clone();
    pending["status"] = json!("in_progress");
    pending["output"] = json!([]);
    pending["usage"] = Value::Null;
    let mut pending_item = item.clone();
    pending_item["status"] = json!("in_progress");
    pending_item["content"] = json!([]);
    let mut empty_part = part.clone();
    empty_part["text"] = json!("");
    let mut events = Vec::new();
    let mut push = |kind: &str, mut data: Value| {
        data["type"] = json!(kind);
        data["sequence_number"] = json!(events.len());
        events.push(Event::default().event(kind).data(data.to_string()));
    };
    push("response.created", json!({"response":pending}));
    push("response.in_progress", json!({"response":pending}));
    push(
        "response.output_item.added",
        json!({"output_index":0,"item":pending_item}),
    );
    push(
        "response.content_part.added",
        json!({"item_id":item["id"],"output_index":0,"content_index":0,"part":empty_part}),
    );
    for text in chunks(part["text"].as_str().unwrap()) {
        push(
            "response.output_text.delta",
            json!({"item_id":item["id"],"output_index":0,"content_index":0,"delta":text,"logprobs":[]}),
        );
    }
    push(
        "response.output_text.done",
        json!({"item_id":item["id"],"output_index":0,"content_index":0,"text":part["text"],"logprobs":[]}),
    );
    push(
        "response.content_part.done",
        json!({"item_id":item["id"],"output_index":0,"content_index":0,"part":part}),
    );
    push(
        "response.output_item.done",
        json!({"output_index":0,"item":item}),
    );
    push("response.completed", json!({"response":response}));
    send(events)
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

    const REPLY: &str = "Hello 세계 🌍\nA longer fixed reply with  spaces.";

    async fn request(path: &str, payload: Value, auth: bool) -> (StatusCode, String, String) {
        let data: TestData =
            serde_json::from_value(json!({"models":[{"id":"test","reply":REPLY,"embedding":[1]}]}))
                .unwrap();
        let mut request = Request::builder()
            .method("POST")
            .uri(path)
            .header("Content-Type", "application/json");
        if auth {
            request = request.header("Authorization", "Bearer test-key");
        }
        let response = app::router("test-key".into(), data)
            .oneshot(request.body(Body::from(payload.to_string())).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let content_type = response.headers()["content-type"]
            .to_str()
            .unwrap()
            .to_owned();
        let text = String::from_utf8(
            to_bytes(response.into_body(), 65536)
                .await
                .unwrap()
                .to_vec(),
        )
        .unwrap();
        (status, content_type, text)
    }

    fn frames(text: &str) -> Vec<(Option<String>, String)> {
        assert!(text.ends_with("\n\n"));
        text.split("\n\n")
            .filter(|frame| !frame.is_empty())
            .map(|frame| {
                let event = frame.lines().find_map(|line| {
                    line.strip_prefix("event:")
                        .map(|value| value.trim().to_owned())
                });
                let data = frame
                    .lines()
                    .find_map(|line| {
                        line.strip_prefix("data:")
                            .map(|value| value.trim().to_owned())
                    })
                    .unwrap();
                (event, data)
            })
            .collect()
    }

    #[tokio::test]
    async fn chat_stream_rebuilds_reply_and_optional_usage() {
        for include_usage in [false, true] {
            let payload = json!({"model":"test","messages":[{"role":"user","content":"Hi there"}],"stream":true,"stream_options":{"include_usage":include_usage}});
            let (status, content_type, text) = request("/v1/chat/completions", payload, true).await;
            assert_eq!(status, StatusCode::OK);
            assert_eq!(content_type, "text/event-stream");
            let frames = frames(&text);
            assert_eq!(frames.last().unwrap().1, "[DONE]");
            let chunks: Vec<Value> = frames[..frames.len() - 1]
                .iter()
                .map(|(event, data)| {
                    assert!(event.is_none());
                    serde_json::from_str(data).unwrap()
                })
                .collect();
            assert_eq!(chunks[0]["choices"][0]["delta"]["role"], "assistant");
            let mut rebuilt = String::new();
            let mut finished = 0;
            for chunk in &chunks {
                assert_eq!(chunk["object"], "chat.completion.chunk");
                assert_eq!(chunk["id"], chunks[0]["id"]);
                assert_eq!(chunk["created"], chunks[0]["created"]);
                assert_eq!(chunk["model"], "test");
                if let Some(content) = chunk["choices"][0]["delta"]["content"].as_str() {
                    rebuilt.push_str(content);
                }
                if chunk["choices"][0]["finish_reason"] == "stop" {
                    finished += 1;
                }
            }
            assert_eq!(rebuilt, REPLY);
            assert_eq!(finished, 1);
            if include_usage {
                assert_eq!(chunks.last().unwrap()["choices"], json!([]));
                assert_eq!(chunks.last().unwrap()["usage"]["prompt_tokens"], 2);
            } else {
                assert!(chunks.iter().all(|chunk| chunk["usage"].is_null()));
            }
        }
    }

    #[tokio::test]
    async fn response_stream_has_ordered_lifecycle_and_complete_output() {
        let payload = json!({"model":"test","input":"Hi there","stream":true});
        let (status, content_type, text) = request("/v1/responses", payload, true).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(content_type, "text/event-stream");
        assert!(!text.contains("[DONE]"));
        let frames = frames(&text);
        let events: Vec<Value> = frames
            .iter()
            .enumerate()
            .map(|(sequence, (event, data))| {
                let data: Value = serde_json::from_str(data).unwrap();
                assert_eq!(event.as_deref(), data["type"].as_str());
                assert_eq!(data["sequence_number"], sequence);
                data
            })
            .collect();
        assert_eq!(events[0]["type"], "response.created");
        assert_eq!(events[0]["response"]["status"], "in_progress");
        assert_eq!(events[0]["response"]["output"], json!([]));
        assert!(events[0]["response"]["usage"].is_null());
        assert_eq!(events[1]["type"], "response.in_progress");
        assert_eq!(events[2]["type"], "response.output_item.added");
        assert_eq!(events[2]["item"]["status"], "in_progress");
        assert_eq!(events[3]["type"], "response.content_part.added");
        let deltas: Vec<&Value> = events
            .iter()
            .filter(|event| event["type"] == "response.output_text.delta")
            .collect();
        assert!(deltas.len() > 1);
        let rebuilt: String = deltas
            .iter()
            .map(|event| event["delta"].as_str().unwrap())
            .collect();
        assert_eq!(rebuilt, REPLY);
        let completed = events.last().unwrap();
        assert_eq!(completed["type"], "response.completed");
        let response = &completed["response"];
        assert_eq!(response["id"], events[0]["response"]["id"]);
        assert_eq!(response["status"], "completed");
        for delta in deltas {
            assert_eq!(delta["item_id"], response["output"][0]["id"]);
            assert_eq!(delta["output_index"], 0);
            assert_eq!(delta["content_index"], 0);
        }
        let tail = events.len() - 4;
        assert_eq!(events[tail]["type"], "response.output_text.done");
        assert_eq!(events[tail]["text"], REPLY);
        assert_eq!(events[tail + 1]["type"], "response.content_part.done");
        assert_eq!(events[tail + 2]["type"], "response.output_item.done");
        assert_eq!(events[tail + 2]["item"], response["output"][0]);
        let (_, _, plain) = request(
            "/v1/responses",
            json!({"model":"test","input":"Hi there"}),
            true,
        )
        .await;
        let plain: Value = serde_json::from_str(&plain).unwrap();
        assert_eq!(
            response["output"][0]["content"],
            plain["output"][0]["content"]
        );
        assert_eq!(response["usage"], plain["usage"]);
    }

    #[tokio::test]
    async fn stream_validation_errors_stay_json() {
        for (path, base) in [
            (
                "/v1/chat/completions",
                json!({"model":"test","messages":[{"role":"user","content":"Hi"}],"stream":true}),
            ),
            (
                "/v1/responses",
                json!({"model":"test","input":"Hi","stream":true}),
            ),
        ] {
            let (status, content_type, _) = request(path, base.clone(), false).await;
            assert_eq!(status, StatusCode::UNAUTHORIZED);
            assert_eq!(content_type, "application/json");
            for (model, expected) in [
                (json!("missing"), StatusCode::NOT_FOUND),
                (Value::Null, StatusCode::BAD_REQUEST),
            ] {
                let mut payload = base.clone();
                payload["model"] = model;
                let (status, content_type, body) = request(path, payload, true).await;
                assert_eq!(status, expected);
                assert_eq!(content_type, "application/json");
                let body: Value = serde_json::from_str(&body).unwrap();
                assert!(body["error"]["message"].is_string());
            }
        }
        for (stream, options) in [
            (json!(false), json!({"include_usage":true})),
            (json!(true), json!({"include_usage":"bad"})),
            (json!(true), json!({"unknown":true})),
        ] {
            let (status,_,body) = request("/v1/chat/completions",json!({"model":"test","messages":[{"role":"user","content":"Hi"}],"stream":stream,"stream_options":options}),true).await;
            assert_eq!(status, StatusCode::BAD_REQUEST);
            let body: Value = serde_json::from_str(&body).unwrap();
            assert_eq!(body["error"]["param"], "stream_options");
        }
    }
}
