use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CoreError {
    #[error("agent writes disabled")]
    AgentWriteDisabled,
    #[error("agent request id reused with different content")]
    AgentRequestConflict,
    #[error("agent warehouse identity changed")]
    AgentWarehouseChanged,
    #[error("unsupported agent contract version")]
    AgentVersion,
    #[error("agent data exceeds supported limits")]
    AgentLimit,
    #[error("export exceeds supported limits")]
    ExportLimit,
    #[error("spreadsheet encoding failed")]
    ExportEncoding,
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
    #[error("interview round is referenced by an event")]
    EventRoundInUse,
    #[error("batch workflow definitions conflict")]
    BatchConflict,
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
    #[error("document target conflicts with an existing file or index")]
    DocumentNameConflict,
    #[error("interrupted document rename requires recovery")]
    DocumentRenameRecovery,
    #[error("interrupted document trash operation requires recovery")]
    DocumentTrashRecovery,
    #[error("copied files could not be verified")]
    CopyVerification,
    #[error("interrupted copy requires recovery")]
    CopyRecovery,
    #[error("unsafe path was rejected")]
    UnsafePath,
    #[error("confirmation is invalid")]
    InvalidConfirmation,
    #[error("database backup is invalid or incompatible")]
    BackupInvalid,
    #[error("database-only restore cannot replay pending file operations")]
    BackupPendingOperations,
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
            CoreError::AgentWriteDisabled => Self {
                code: "AGENT_WRITE_DISABLED",
                message: "Agent 写入未开启。只能由用户在当前仓库设置中明确开启，Agent 不能自行授权。",
                retryable: false,
            },
            CoreError::AgentRequestConflict => Self {
                code: "AGENT_REQUEST_CONFLICT",
                message: "请求 ID 已被其他内容使用。请核对原请求及审计；仅相同请求可以安全重试。",
                retryable: false,
            },
            CoreError::AgentWarehouseChanged => Self {
                code: "AGENT_WAREHOUSE_CHANGED",
                message: "配置路径中的仓库身份已改变，已停止查询。请确认目标仓库后重新建立 Agent 连接。",
                retryable: false,
            },
            CoreError::AgentVersion => Self {
                code: "AGENT_VERSION_UNSUPPORTED",
                message: "不支持该 Agent 请求版本。请使用 --help 核对当前契约版本；未修改仓库。",
                retryable: false,
            },
            CoreError::AgentLimit => Self {
                code: "AGENT_LIMIT",
                message: "Agent 输入/数据超过限制（请求 64 KiB、每类 10000 项、完整 JSON 64 MiB）。未截断输出或修改原数据。",
                retryable: false,
            },
            CoreError::ExportLimit => Self {
                code: "EXPORT_LIMIT",
                message: "导出超过限制：最多 10000 条、256 列、32 MiB 原文；XLSX 单格最多 32767 个字符。请缩小范围、减少字段，或将超长文本改用 CSV 导出。未覆盖原数据。",
                retryable: false,
            },
            CoreError::ExportEncoding => Self {
                code: "EXPORT_ENCODING",
                message: "无法生成表格，请检查导出字段后重试。原数据未修改；失败暂存保留。",
                retryable: true,
            },
            CoreError::EventRoundInUse => Self {
                code: "EVENT_ROUND_IN_USE",
                message: "该轮次已被招聘事件关联，请先在待办与日程中解除事件关联，再删除轮次。",
                retryable: true,
            },
            CoreError::BatchConflict => Self {
                code: "BATCH_WORKFLOW_CONFLICT",
                message: "选中投递的流程不兼容：阶段缺失、同名不同定义、分类冲突或超过 100 项。整批未保存，请分别调整记录或缩小范围后重新预览。",
                retryable: false,
            },
            CoreError::BackupInvalid => Self {
                code: "BACKUP_INVALID",
                message: "备份缺失、校验失败或版本不兼容。未覆盖当前仓库；失败暂存内容保留，请检查后重试或选择其他备份。",
                retryable: false,
            },
            CoreError::BackupPendingOperations => Self {
                code: "BACKUP_PENDING_OPERATIONS",
                message: "存在未完成的文件操作，暂不能完整备份、迁移或从该快照恢复。请保留备份和原文件，先处理原仓库的文件恢复问题。",
                retryable: false,
            },
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
            CoreError::DocumentNameConflict => Self {
                code: "DOCUMENT_NAME_CONFLICT",
                message: "目标名称已有文件或保留索引，未覆盖。请选择其他名称；仅改变英文字母大小写时请先改为一个临时名称。",
                retryable: true,
            },
            CoreError::DocumentRenameRecovery => Self {
                code: "DOCUMENT_RENAME_RECOVERY",
                message: "附件重命名尚未完成，原文件和日志已保留。请关闭占用程序后重开仓库；若仍失败，可只读打开查看文件操作诊断。请勿覆盖或删除任一候选文件。",
                retryable: true,
            },
            CoreError::DocumentTrashRecovery => Self {
                code: "DOCUMENT_TRASH_RECOVERY",
                message: "附件回收站操作尚未完成，文件和恢复日志已保留。请关闭占用程序后重开仓库；若仍失败，可只读打开查看诊断。不要覆盖候选文件。",
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
