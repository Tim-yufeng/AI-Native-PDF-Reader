use serde::Serialize;

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AppError {
    pub code: &'static str,
    pub message: &'static str,
}
impl AppError {
    pub fn denied() -> Self { Self { code: "unauthorized", message: "文件授权已失效，请重新打开。" } }
    pub fn cancelled() -> Self { Self { code: "cancelled", message: "读取已取消。" } }
    pub fn busy() -> Self { Self { code: "busy", message: "已有读取任务或打开的文件过多，请稍后重试。" } }
    pub fn io() -> Self { Self { code: "file_unavailable", message: "无法读取文件，可能被其他程序占用，请重新选择。" } }
    pub fn invalid() -> Self { Self { code: "invalid_pdf", message: "请选择非空、本地且不超过64 MB的PDF文件。" } }
    pub fn changed() -> Self { Self { code: "file_changed", message: "文件已发生变化，请关闭后重新打开。" } }
}
impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { f.write_str(self.message) }
}
impl std::error::Error for AppError {}
pub type Result<T> = std::result::Result<T, AppError>;
