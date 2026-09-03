# OfferTrack

OfferTrack 是一款面向 Windows 10/11 的本地优先求职投递管理桌面应用。投递数据、简历和备份均保存在用户选择的数据仓库中，不要求账号，也不依赖云端服务。

> 当前版本仍是发布前预览版。请先使用虚构数据试用，不要把真实资料的唯一副本交给尚未正式发布的构建。

## 主要功能

- 以表格和详情面板管理投递日期、公司、岗位、链接、进度、标签、备注和自定义字段。
- 支持可保存视图、多条件筛选、排序、分页、键盘单格编辑、批量修改和基础撤销。
- 每条投递拥有独立文件夹，可在文件管理器中直接管理 PDF、Word 等附件。
- 支持自定义招聘流程、辅助状态、多轮面试、流程模板和完整状态历史。
- 提供概览、提醒、通用及关联待办、招聘事件、综合日程和 XLSX/CSV 导出。
- 提供数据库快照、包含附件的完整备份、独立恢复、仓库迁移及分区回收站。
- 提供离线帮助、浅色/深色主题以及面向本地 Agent 的 JSON/JSONL 快照、CLI 和 stdio MCP。

OfferTrack 默认不启动本地服务器、不监听端口，也不在后台联网。网页链接仅在用户操作后交给系统浏览器；附件可交给默认应用、Windows“选择其他应用”或资源管理器打开。

## 数据与安全边界

- 数据仓库由用户选择，数据库、投递文件夹、备份和 Agent 快照都在本地保存。
- 同一仓库只允许一个写入实例；仓库被占用时可以只读打开。
- 投递和附件删除先进入当前仓库的固定回收站。永久删除必须二次确认，后端不接受任意删除路径，并拒绝路径穿越、符号链接和目录联接点。
- 删除、恢复、目录规范化、附件重命名、新建和完整复制均使用可恢复操作日志。
- 完整恢复只写入全新目录，不覆盖当前活跃仓库；迁移会先生成并验证完整备份。
- Agent 写入默认关闭。启用后，每批写入仍须先备份、校验版本、事务提交并记录审计；Agent 没有任意 SQL、命令、附件修改或删除能力。
- 备份和 Agent 快照未加密，可能包含个人信息和完整长文本，请勿上传到公开仓库或不可信服务。
- 网络盘、同步目录和移动磁盘可能降低 SQLite 与文件操作的可靠性；应用会提示风险，但最终位置由用户决定。

当前数据库 schema 为 `11`，仓库格式、数据库备份、完整备份和 Agent 契约格式均为 `1`。升级后的仓库不应再使用旧构建打开。

## 开发环境

需要：

- Node.js `>=22.12 <25`；
- pnpm `11.x`，仓库锁定为 `11.19.0`；
- Rust `1.98.0`，由 `rust-toolchain.toml` 锁定；
- Visual Studio C++ Build Tools，包含“使用 C++ 的桌面开发”和 Windows SDK；
- Microsoft Edge WebView2 Runtime。

安装依赖：

```powershell
pnpm install --frozen-lockfile
```

启动桌面开发版本：

```powershell
pnpm tauri dev
```

## 检查与构建

执行前端、发布脚本、许可证清单和生产构建检查：

```powershell
pnpm check
```

执行 Rust 检查：

```powershell
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml
```

生成普通 Windows 开发构建：

```powershell
pnpm tauri build --no-bundle
```

产物位于 `src-tauri/target/release/offertrack.exe` 和同目录的 `offertrack-cli.exe`。普通开发构建不得直接作为公开发布工件。

## 发布准备

发布前重新生成并检查第三方许可证，随后执行路径清理构建和安全审计：

```powershell
pnpm license:generate
pnpm license:check
pnpm release:build
pnpm release:audit
```

选择源码仓库外一个已经存在的空目录生成白名单便携包：

```powershell
pnpm release:portable C:\path\to\empty-release-output
```

发布构建会移除二进制中的本机源码路径。打包器只接受桌面程序、CLI、README、MIT License、第三方声明、变更记录和安全说明，并生成版本清单、包内文件哈希、确定性 ZIP 和 ZIP 外部 SHA-256。它拒绝仓库内输出、链接或联接目录、已有同名目标以及额外数据库、备份、日志、PDB 和源码文件。

这些自动化检查不能替代 Windows 原生人工验收、第三方许可证人工复核或明确的 Release 授权。

## 使用文档

- [用户指南](docs/user-guide/README.md)
- [本地 Agent 接口](docs/agent-api.md)
- [备份与恢复格式](docs/backup-format.md)
- [版本变更](CHANGELOG.md)
- [安全政策](SECURITY.md)
- [第三方许可证](THIRD_PARTY_NOTICES.md)

## 许可证

OfferTrack 使用 [MIT License](LICENSE)。第三方组件继续遵循各自许可证，详见 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)。
