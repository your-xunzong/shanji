use thiserror::Error;

pub type AppResult<T> = Result<T, AppError>;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("{0}")]
    Validation(String),
    #[error("本地数据库操作失败")]
    Database(#[from] rusqlite::Error),
    #[error("本地数据文件操作失败")]
    Io(#[from] std::io::Error),
    #[error("本地设置数据损坏")]
    SettingsData(#[from] serde_json::Error),
    #[error("找不到事项")]
    ItemNotFound,
    #[error("系统集成功能不可用：{0}")]
    SystemIntegration(String),
}

impl From<AppError> for String {
    fn from(value: AppError) -> Self {
        value.to_string()
    }
}
