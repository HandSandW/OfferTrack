use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CoreError {
    #[error("warehouse is already locked")]
    WarehouseLocked,
    #[error("warehouse directory is not empty")]
    WarehouseNotEmpty,
    #[error("warehouse metadata is missing")]
    MetadataMissing,
    #[error("warehouse metadata is invalid")]
    MetadataInvalid,
    #[error("warehouse format is unsupported")]
    UnsupportedFormat,
    #[error("warehouse database is missing")]
    DatabaseMissing,
    #[error("warehouse database is invalid")]
    DatabaseInvalid,
    #[error("local storage operation failed")]
    Storage,
    #[error("application state is unavailable")]
    StateUnavailable,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppErrorPayload {
    pub code: &'static str,
    pub message: &'static str,
    pub retryable: bool,
}

impl From<CoreError> for AppErrorPayload {
    fn from(error: CoreError) -> Self {
        match error {
            CoreError::WarehouseLocked => Self {
                code: "WAREHOUSE_LOCKED",
                message: "该数据仓库正被另一个 OfferTrack 实例写入。你可以关闭另一实例，或改为只读打开。",
                retryable: true,
            },
            CoreError::WarehouseNotEmpty => Self {
                code: "WAREHOUSE_NOT_EMPTY",
                message: "新建数据仓库需要使用空文件夹，以免覆盖已有文件。",
                retryable: true,
            },
            CoreError::MetadataMissing => Self {
                code: "WAREHOUSE_METADATA_MISSING",
                message: "所选文件夹不是有效的 OfferTrack 数据仓库：缺少 warehouse.json。",
                retryable: true,
            },
            CoreError::MetadataInvalid => Self {
                code: "WAREHOUSE_METADATA_INVALID",
                message: "数据仓库元数据无法读取或校验失败。",
                retryable: false,
            },
            CoreError::UnsupportedFormat => Self {
                code: "WAREHOUSE_FORMAT_UNSUPPORTED",
                message: "此数据仓库由不兼容的 OfferTrack 版本创建。",
                retryable: false,
            },
            CoreError::DatabaseMissing => Self {
                code: "WAREHOUSE_DATABASE_MISSING",
                message: "数据仓库缺少 offertrack.sqlite。",
                retryable: false,
            },
            CoreError::DatabaseInvalid => Self {
                code: "WAREHOUSE_DATABASE_INVALID",
                message: "数据仓库数据库无法打开、迁移或校验。",
                retryable: false,
            },
            CoreError::Storage => Self {
                code: "STORAGE_OPERATION_FAILED",
                message: "无法访问所选位置。请检查文件夹权限和磁盘状态。",
                retryable: true,
            },
            CoreError::StateUnavailable => Self {
                code: "APP_STATE_UNAVAILABLE",
                message: "OfferTrack 内部状态暂时不可用，请重新启动应用。",
                retryable: true,
            },
        }
    }
}
