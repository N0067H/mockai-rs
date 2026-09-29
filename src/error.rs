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
    message: String,
    #[serde(rename = "type")]
    kind: &'static str,
    param: Option<String>,
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

    pub fn model_not_found(model: &str) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            body: ErrorBody {
                message: format!("Model '{model}' not found."),
                kind: "invalid_request_error",
                param: Some("model".into()),
                code: Some("model_not_found"),
            },
        }
    }

    pub fn invalid_request(message: impl Into<String>, param: Option<&str>) -> Self {
        Self::request_error(StatusCode::BAD_REQUEST, message, param)
    }

    pub fn request_error(
        status: StatusCode,
        message: impl Into<String>,
        param: Option<&str>,
    ) -> Self {
        Self {
            status,
            body: ErrorBody {
                message: message.into(),
                kind: "invalid_request_error",
                param: param.map(str::to_owned),
                code: None,
            },
        }
    }

    fn new(status: StatusCode, message: &'static str, code: Option<&'static str>) -> Self {
        Self {
            status,
            body: ErrorBody {
                message: message.into(),
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
