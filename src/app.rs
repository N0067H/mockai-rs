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

pub fn router(api_key: String) -> Router {
    Router::new()
        .route("/", get(|| async { "mockai-rs\n" }))
        .fallback(|| async { ApiError::not_found() })
        .method_not_allowed_fallback(|| async { ApiError::method_not_allowed() })
        .layer(middleware::from_fn_with_state(
            Arc::new(api_key),
            authenticate,
        ))
}

async fn authenticate(
    State(api_key): State<Arc<String>>,
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
        if headers.next().is_some() || token != Some(api_key.as_str()) {
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

    use super::router;

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

        for path in ["/v1", "/v1/models", "/v1/missing"] {
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
