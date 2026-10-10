use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use tmuxide_git::GitError;
use tmuxide_tmux::TmuxError;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("{0}")]
    BadRequest(String),
    #[error("{0}")]
    NotFound(String),
    #[error("{0}")]
    Conflict(String),
    #[error("{0}")]
    Tmux(#[from] TmuxError),
    #[error("Git：{0}")]
    Git(#[from] GitError),
    #[error("{0}")]
    Core(#[from] tmuxide_core::CoreError),
    #[error("文件读写失败：{0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Other(String),
}

impl AppError {
    fn status(&self) -> StatusCode {
        match self {
            AppError::BadRequest(_) | AppError::Core(_) => StatusCode::BAD_REQUEST,
            AppError::NotFound(_) => StatusCode::NOT_FOUND,
            AppError::Conflict(_) => StatusCode::CONFLICT,
            AppError::Tmux(TmuxError::BadName(_)) => StatusCode::BAD_REQUEST,
            AppError::Tmux(TmuxError::NoSession(_)) => StatusCode::NOT_FOUND,
            AppError::Tmux(TmuxError::Exists(_)) => StatusCode::CONFLICT,
            AppError::Git(GitError::Conflict(_)) => StatusCode::CONFLICT,
            AppError::Git(GitError::NoRemote) => StatusCode::BAD_REQUEST,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let status = self.status();
        if status.is_server_error() {
            tracing::warn!("{self}");
        }
        (status, Json(serde_json::json!({ "error": self.to_string() }))).into_response()
    }
}

pub type AppResult<T> = Result<T, AppError>;
