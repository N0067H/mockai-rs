use std::sync::Arc;

use axum::{
    Router,
    extract::{Request, State},
    http::header::AUTHORIZATION,
    middleware::{self, Next},
    response::Response,
    routing::get,
};

use crate::error::ApiError;
use crate::test_data::TestData;

pub(crate) struct AppState {
    api_key: String,
    pub(crate) test_data: TestData,
}

pub fn router(api_key: String, test_data: TestData) -> Router {
    let state = Arc::new(AppState { api_key, test_data });
    Router::new()
        .route("/", get(|| async { "mockai-rs\n" }))
        .route("/v1/models", get(crate::models::list))
        .route("/v1/models/{model}", get(crate::models::get))
        .fallback(|| async { ApiError::not_found() })
        .method_not_allowed_fallback(|| async { ApiError::method_not_allowed() })
        .layer(middleware::from_fn_with_state(state.clone(), authenticate))
        .with_state(state)
}

async fn authenticate(
    State(state): State<Arc<AppState>>,
    request: Request,
    next: Next,
) -> Result<Response, ApiError> {
    let path = request.uri().path();
    if path == "/v1" || path.starts_with("/v1/") {
        let mut headers = request.headers().get_all(AUTHORIZATION).iter();
        let token = headers
            .next()
            .and_then(|header| header.to_str().ok())
            .and_then(|value| value.split_once(' '))
            .filter(|(scheme, _)| scheme.eq_ignore_ascii_case("Bearer"))
            .map(|(_, token)| token);
        if headers.next().is_some() || token != Some(state.api_key.as_str()) {
            return Err(ApiError::unauthorized());
        }
    }
    Ok(next.run(request).await)
}

#[cfg(test)]
mod tests {
    use axum::{
        body::{Body, to_bytes},
        http::{Request, StatusCode},
    };
    use serde_json::{Value, json};
    use tower::ServiceExt;

    fn router(api_key: String) -> axum::Router {
        super::router(api_key, crate::test_data::TestData::load(None).unwrap())
    }

    async fn model_request(app: axum::Router, method: &str, path: &str) -> (StatusCode, Value) {
        let response = app
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(path)
                    .header("Authorization", "Bearer test-key")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status();
        assert_eq!(response.headers()["content-type"], "application/json");
        let body =
            serde_json::from_slice(&to_bytes(response.into_body(), 4096).await.unwrap()).unwrap();
        (status, body)
    }

    #[tokio::test]
    async fn model_list_and_lookup_use_configured_data() {
        let data = serde_json::from_value(json!({"models": [
            {"id": "custom-model", "created": 123, "owned_by": "test-owner", "reply": "Private reply", "embedding": [1]},
            {"id": "other-model", "reply": "Other", "embedding": [2]}
        ]})).unwrap();
        let app = super::router("test-key".into(), data);
        let (status, list) = model_request(app.clone(), "GET", "/v1/models").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            list,
            json!({"object": "list", "data": [
                {"id": "custom-model", "object": "model", "created": 123, "owned_by": "test-owner"},
                {"id": "other-model", "object": "model", "created": 0, "owned_by": "mockai-rs"}
            ]})
        );
        for (path, index) in [
            ("/v1/models/custom-model", 0),
            ("/v1/models/other%2Dmodel", 1),
        ] {
            let (status, model) = model_request(app.clone(), "GET", path).await;
            assert_eq!(status, StatusCode::OK);
            assert_eq!(model, list["data"][index]);
        }
        let (status, _) = model_request(app, "GET", "/v1/models/mock-model").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn default_model_and_missing_model_errors() {
        let app = router("test-key".into());
        let (status, model) = model_request(app.clone(), "GET", "/v1/models/mock-model").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            model,
            json!({"id":"mock-model", "object":"model", "created":0, "owned_by":"mockai-rs"})
        );
        let (status, body) = model_request(app.clone(), "GET", "/v1/models/missing").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(
            body,
            json!({"error":{"message":"Model 'missing' not found.", "type":"invalid_request_error", "param":"model", "code":"model_not_found"}})
        );
        for path in ["/v1/models", "/v1/models/mock-model"] {
            let (status, body) = model_request(app.clone(), "POST", path).await;
            assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED);
            assert_eq!(body["error"]["type"], "invalid_request_error");
            let response = app
                .clone()
                .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        }
    }

    #[tokio::test]
    async fn api_auth_and_error_shapes() {
        for header in [
            None,
            Some("Bearer wrong"),
            Some("Basic mock-api-key"),
            Some("Bearer "),
            Some("Bearer mock-api-key extra"),
        ] {
            let mut request = Request::builder().uri("/v1/models");
            if let Some(header) = header {
                request = request.header("Authorization", header);
            }
            let response = router("mock-api-key".into())
                .oneshot(request.body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
            assert_eq!(response.headers()["content-type"], "application/json");
            let body: Value =
                serde_json::from_slice(&to_bytes(response.into_body(), 4096).await.unwrap())
                    .unwrap();
            assert_eq!(
                body,
                json!({"error": {"message": "A valid Bearer test key is required.", "type": "invalid_request_error", "param": null, "code": "invalid_api_key"}})
            );
        }

        for path in ["/v1", "/v1/missing"] {
            let response = router("custom-key".into())
                .oneshot(
                    Request::builder()
                        .uri(path)
                        .header("Authorization", "bearer custom-key")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::NOT_FOUND);
            let body: Value =
                serde_json::from_slice(&to_bytes(response.into_body(), 4096).await.unwrap())
                    .unwrap();
            assert_eq!(body["error"]["type"], "invalid_request_error");
            assert_eq!(body["error"]["code"], Value::Null);
        }
    }

    #[tokio::test]
    async fn root_is_public_and_errors_are_json() {
        for (method, path, status) in [
            ("GET", "/", StatusCode::OK),
            ("POST", "/", StatusCode::METHOD_NOT_ALLOWED),
            ("GET", "/missing", StatusCode::NOT_FOUND),
        ] {
            let response = router("test-key".into())
                .oneshot(
                    Request::builder()
                        .method(method)
                        .uri(path)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), status);
            if status != StatusCode::OK {
                let body: Value =
                    serde_json::from_slice(&to_bytes(response.into_body(), 4096).await.unwrap())
                        .unwrap();
                assert!(body["error"]["message"].is_string());
                assert!(body["error"]["param"].is_null());
            }
        }
    }

    #[tokio::test]
    async fn duplicate_auth_headers_are_rejected() {
        let response = router("test-key".into())
            .oneshot(
                Request::builder()
                    .uri("/v1/models")
                    .header("Authorization", "Bearer test-key")
                    .header("Authorization", "Bearer test-key")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }
}
