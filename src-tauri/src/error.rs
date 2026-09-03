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
    #[error("warehouse operation is in progress")]
    OperationBusy,
    #[error("warehouse is read-only")]
    ReadOnlyWarehouse,
    #[error("warehouse is not open")]
    WarehouseNotOpen,
    #[error("requested entity was not found")]
    NotFound,
    #[error("input validation failed")]
    Validation,
    #[error("record has changed")]
    RevisionConflict,
    #[error("file operation failed")]
    FileOperation,
    #[error("file or directory is missing")]
    FileMissing,
    #[error("file or directory is in use")]
    FileBusy,
    #[error("file access denied")]
    FileAccessDenied,
    #[error("unexpected filesystem entry type")]
    FileTypeMismatch,
    #[error("copied files could not be verified")]
    CopyVerification,
    #[error("interrupted copy requires recovery")]
    CopyRecovery,
    #[error("unsafe path was rejected")]
    UnsafePath,
    #[error("confirmation is invalid")]
    InvalidConfirmation,
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
            CoreError::ReadOnlyWarehouse => Self {
                code: "WAREHOUSE_READ_ONLY",
                message: "当前数据仓库以只读方式打开，不能保存更改。",
                retryable: false,
            },
            CoreError::OperationBusy => Self {
                code: "WAREHOUSE_OPERATION_BUSY",
                message: "当前仓库正在执行操作，请等待完成后重试。",
                retryable: true,
            },
            CoreError::FileMissing => Self {
                code: "FILE_MISSING",
                message: "文件或文件夹已不存在，可能已在资源管理器中移动或改名。记录仍保留；请检查原位置并重新扫描，不会自动重建或删除数据。",
                retryable: true,
            },
            CoreError::FileBusy => Self {
                code: "FILE_BUSY",
                message: "文件或文件夹正在被其他程序占用。请关闭正在编辑附件的程序后重试，不会强制解除占用。",
                retryable: true,
            },
            CoreError::FileAccessDenied => Self {
                code: "FILE_ACCESS_DENIED",
                message: "没有访问文件或目录的权限。请检查权限后重试；访问失败不代表文件已丢失。",
                retryable: true,
            },
            CoreError::FileTypeMismatch => Self {
                code: "FILE_TYPE_MISMATCH",
                message: "预期的文件或目录已被其他类型的项目替代。请检查原位置，不会覆盖该项目。",
                retryable: true,
            },
            CoreError::WarehouseNotOpen => Self {
                code: "WAREHOUSE_NOT_OPEN",
                message: "请先创建或打开一个 OfferTrack 数据仓库。",
                retryable: true,
            },
            CoreError::NotFound => Self {
                code: "NOT_FOUND",
                message: "请求的记录不存在或已被移除。",
                retryable: false,
            },
            CoreError::Validation => Self {
                code: "VALIDATION_FAILED",
                message: "输入内容未通过校验，请检查必填字段和格式。",
                retryable: false,
            },
            CoreError::RevisionConflict => Self {
                code: "REVISION_CONFLICT",
                message: "该记录已在其他位置更新，请刷新后重试。",
                retryable: true,
            },
            CoreError::FileOperation => Self {
                code: "FILE_OPERATION_FAILED",
                message: "文件操作未完成；原有记录和文件已尽可能保持不变。",
                retryable: true,
            },
            CoreError::CopyVerification => Self {
                code: "COPY_VERIFICATION_FAILED",
                message: "复制校验未通过，可能有文件正在修改。未创建新记录，请关闭编辑文件的程序后重试。",
                retryable: true,
            },
            CoreError::CopyRecovery => Self {
                code: "COPY_RECOVERY_REQUIRED",
                message: "未完成的新建或复制暂时无法恢复，可能存在文件占用、路径变化或目录身份不明。请释放占用后重开仓库；若持续失败，可只读打开查看。不要手动清理复制临时目录，原始投递未被修改。",
                retryable: true,
            },
            CoreError::UnsafePath => Self {
                code: "UNSAFE_PATH_REJECTED",
                message: "路径不安全或包含符号链接、目录联接点等重解析点，操作已被拒绝。请检查原位置；不会跟随链接或自动修复。",
                retryable: false,
            },
            CoreError::InvalidConfirmation => Self {
                code: "INVALID_CONFIRMATION",
                message: "确认已失效，请重新发起操作。",
                retryable: true,
            },
        }
    }
}

/// Classify without leaking OS error messages containing absolute user paths.
pub(crate) fn file_error(error: std::io::Error) -> CoreError {
    #[cfg(windows)]
    if matches!(error.raw_os_error(), Some(32 | 33)) {
        return CoreError::FileBusy;
    }
    match error.kind() {
        std::io::ErrorKind::NotFound => CoreError::FileMissing,
        std::io::ErrorKind::PermissionDenied => CoreError::FileAccessDenied,
        std::io::ErrorKind::NotADirectory | std::io::ErrorKind::IsADirectory => {
            CoreError::FileTypeMismatch
        }
        _ => CoreError::FileOperation,
    }
}
