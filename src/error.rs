use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Serialize;

pub struct ApiError {
    status: StatusCode,
    body: ErrorBody,
}

#[derive(Serialize)]
struct ErrorBody {
    message: &'static str,
    #[serde(rename = "type")]
    kind: &'static str,
    param: Option<&'static str>,
    code: Option<&'static str>,
}

#[derive(Serialize)]
struct ErrorResponse {
    error: ErrorBody,
}

impl ApiError {
    pub fn unauthorized() -> Self {
        Self::new(
            StatusCode::UNAUTHORIZED,
            "A valid Bearer test key is required.",
            Some("invalid_api_key"),
        )
    }

    pub fn not_found() -> Self {
        Self::new(StatusCode::NOT_FOUND, "Endpoint not found.", None)
    }

    pub fn method_not_allowed() -> Self {
        Self::new(StatusCode::METHOD_NOT_ALLOWED, "Method not allowed.", None)
    }

    fn new(status: StatusCode, message: &'static str, code: Option<&'static str>) -> Self {
        Self {
            status,
            body: ErrorBody {
                message,
                kind: "invalid_request_error",
                param: None,
                code,
            },
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, Json(ErrorResponse { error: self.body })).into_response()
    }
}
