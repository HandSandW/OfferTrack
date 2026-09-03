import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { App } from "./App";
import { OfferTrackError } from "./lib/tauri";
import { applicationFixture } from "./test/applicationFixture";
import { workflowFixture } from "./test/workflowFixture";
import { overviewFixture } from "./test/productivityFixture";

const mocks = vi.hoisted(() => ({
  getStartupState: vi.fn(),
  getOverview: vi.fn(),
  checkAgentSnapshot: vi.fn(),
  listRecruitmentEvents: vi.fn(),
  createWarehouse: vi.fn(),
  openWarehouse: vi.fn(),
  closeWarehouse: vi.fn(),
  listApplications: vi.fn(),
  getApplication: vi.fn(),
  listWorkflowTemplates: vi.fn(),
  getWorkflowTemplate: vi.fn(),
  listFieldDefinitions: vi.fn(),
  listApplicationViews: vi.fn(),
  getApplicationPageSize: vi.fn(),
  listUnlinkedFolders: vi.fn(),
  migrateWarehouse: vi.fn(),
  selectDirectory: vi.fn(),
  selectFullBackup: vi.fn(),
  previewExternalDatabaseBackup: vi.fn(),
  restoreExternalDatabaseBackup: vi.fn(),
  openHelp: vi.fn(),
}));

vi.mock("./features/help/api", () => ({ helpApi: { open: mocks.openHelp } }));

vi.mock("./lib/tauri", async () => {
  const actual =
    await vi.importActual<typeof import("./lib/tauri")>("./lib/tauri");
  return {
    ...actual,
    desktopApi: {
      getStartupState: mocks.getStartupState,
      getOverview: mocks.getOverview,
      checkAgentSnapshot: mocks.checkAgentSnapshot,
      listRecruitmentEvents: mocks.listRecruitmentEvents,
      createWarehouse: mocks.createWarehouse,
      openWarehouse: mocks.openWarehouse,
      closeWarehouse: mocks.closeWarehouse,
      listApplications: mocks.listApplications,
      getApplication: mocks.getApplication,
      listWorkflowTemplates: mocks.listWorkflowTemplates,
      getWorkflowTemplate: mocks.getWorkflowTemplate,
      listFieldDefinitions: mocks.listFieldDefinitions,
      listApplicationViews: mocks.listApplicationViews,
      getApplicationPageSize: mocks.getApplicationPageSize,
      listUnlinkedFolders: mocks.listUnlinkedFolders,
      migrateWarehouse: mocks.migrateWarehouse,
      previewExternalDatabaseBackup: mocks.previewExternalDatabaseBackup,
      restoreExternalDatabaseBackup: mocks.restoreExternalDatabaseBackup,
    },
  };
});

vi.mock("./lib/dialog", () => ({
  selectDirectory: mocks.selectDirectory,
  selectFullBackup: mocks.selectFullBackup,
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn().mockResolvedValue(() => undefined),
}));

describe("OfferTrack app shell", () => {
  it("opens due metric schedule scope and keeps reminder access outside overview", async () => {
    mocks.getStartupState.mockResolvedValue({
      rememberedWarehousePath: null,
      activeWarehouse: {
        warehouseId: "schedule-warehouse",
        formatVersion: 1,
        displayPath: "synthetic-directory",
        accessMode: "write",
        warnings: [],
      },
    });
    mocks.getOverview.mockResolvedValue(
      overviewFixture({
        dueMetrics: [{ label: "已逾期事项", keys: ["event:one"] }],
        schedule: [
          {
            key: "event:one",
            sourceKind: "event",
            sourceId: "one",
            applicationId: null,
            label: "逾期笔试",
            atUtc: "2026-09-01T00:00:00Z",
            startsAtUtc: null,
            finished: false,
            highPriority: false,
          },
          {
            key: "event:two",
            sourceKind: "event",
            sourceId: "two",
            applicationId: null,
            label: "范围外事件",
            atUtc: null,
            startsAtUtc: null,
            finished: false,
            highPriority: false,
          },
        ],
      }),
    );
    render(<App />);
    // Wait for the shared provider before the relatively costly accessible-role query.
    await waitFor(() => expect(mocks.getOverview).toHaveBeenCalled());
    await waitFor(() =>
      expect(screen.queryByText("正在更新概览…")).not.toBeInTheDocument(),
    );
    fireEvent.click(
      await screen.findByRole("button", { name: "已逾期事项 1" }),
    );
    await screen.findByText("逾期笔试");
    expect(screen.queryByText("范围外事件")).not.toBeInTheDocument();
    expect(screen.getByLabelText("应用内提醒")).toBeInTheDocument();
    fireEvent.click(screen.getByText("清除日程范围"));
    await screen.findByText("范围外事件");
    fireEvent.click(screen.getByRole("button", { name: "查看重要提醒（0）" }));
    await screen.findByRole("heading", { name: "求职概览" });
    expect(screen.queryByLabelText("应用内提醒")).not.toBeInTheDocument();
  });
  it("opens an exact overview record set and clears it without changing stored views", async () => {
    mocks.getStartupState.mockResolvedValue({
      rememberedWarehousePath: null,
      activeWarehouse: {
        warehouseId: "overview-warehouse",
        formatVersion: 1,
        displayPath: "synthetic-directory",
        accessMode: "write",
        warnings: [],
      },
    });
    mocks.getOverview.mockResolvedValue(
      overviewFixture({
        metrics: [{ label: "准备投递", ids: ["first", "second"] }],
      }),
    );
    mocks.listApplications.mockResolvedValue([
      applicationFixture({ id: "first", companyName: "第一示例" }),
      applicationFixture({ id: "second", companyName: "第二示例" }),
      applicationFixture({ id: "third", companyName: "范围外示例" }),
    ]);
    render(<App />);
    fireEvent.click(await screen.findByRole("button", { name: "准备投递 2" }));
    await screen.findByText("第一示例");
    expect(screen.getByText("第二示例")).toBeInTheDocument();
    expect(screen.queryByText("范围外示例")).not.toBeInTheDocument();
    fireEvent.click(screen.getByText("清除概览范围"));
    await screen.findByText("范围外示例");
  });

  it.each(["archived", "deleted"])(
    "does not open stale overview detail for a record that became %s",
    async (state) => {
      mocks.getStartupState.mockResolvedValue({
        rememberedWarehousePath: null,
        activeWarehouse: {
          warehouseId: "overview-warehouse",
          formatVersion: 1,
          displayPath: "synthetic-directory",
          accessMode: "write",
          warnings: [],
        },
      });
      mocks.getOverview.mockResolvedValue(
        overviewFixture({ metrics: [{ label: "准备投递", ids: ["stale"] }] }),
      );
      mocks.getApplication.mockResolvedValue(
        applicationFixture({
          id: "stale",
          archivedAtUtc: state === "archived" ? "2026-09-03T03:00:00Z" : null,
          deletedAtUtc: state === "deleted" ? "2026-09-03T03:00:00Z" : null,
        }),
      );
      render(<App />);
      fireEvent.click(
        await screen.findByRole("button", { name: "准备投递 1" }),
      );
      await screen.findByText("该投递已不在当前分区，请刷新概览后重试。");
      expect(mocks.getApplication).toHaveBeenCalledWith("stale");
      expect(screen.queryByLabelText("关闭详情")).not.toBeInTheDocument();
      expect(screen.queryByText("示例公司")).not.toBeInTheDocument();
      expect(screen.getByText("清除概览范围")).toBeEnabled();
    },
  );

  it("offers standalone database recovery when no warehouse is open", async () => {
    mocks.previewExternalDatabaseBackup.mockResolvedValue({
      fingerprint: "hash",
      applicationCount: 1,
      documentCount: 0,
      backup: { warehouseId: "old", schemaVersion: 7 },
    });
    mocks.restoreExternalDatabaseBackup.mockResolvedValue({
      directory: "recovered-snapshot",
      applicationCount: 1,
      documentCount: 0,
    });
    mocks.selectDirectory
      .mockResolvedValueOnce("snapshot-folder")
      .mockResolvedValueOnce("restore-parent");
    render(<App />);
    fireEvent.click(await screen.findByText("备份与迁移"));
    fireEvent.click(await screen.findByText("选择并校验数据库快照目录"));
    fireEvent.click(await screen.findByText("恢复独立数据库快照…"));
    fireEvent.click(await screen.findByText("创建数据库恢复副本"));
    await screen.findByText(/数据库恢复完成：recovered-snapshot/);
    expect(mocks.openWarehouse).not.toHaveBeenCalled();
    expect(mocks.restoreExternalDatabaseBackup).toHaveBeenCalledWith(
      "snapshot-folder",
      "restore-parent",
      "hash",
    );
  });

  afterEach(() => cleanup());

  beforeEach(() => {
    // clearAllMocks leaves mockResolved/RejectedValueOnce queues behind when an
    // earlier test fails midway. Reset only our business mocks, not Tauri listeners.
    for (const mock of Object.values(mocks)) mock.mockReset();
    mocks.checkAgentSnapshot.mockImplementation((warehouse_id: string) =>
      Promise.resolve({
        version: 1,
        warehouse_id,
        state: "current",
        snapshot: null,
        error: null,
        warnings: [],
        published: false,
        checked_at_utc: "2026-09-03T00:00:00Z",
      }),
    );
    mocks.getOverview.mockResolvedValue(overviewFixture());
    mocks.listRecruitmentEvents.mockResolvedValue([]);
    mocks.getStartupState.mockResolvedValue({
      rememberedWarehousePath: null,
      activeWarehouse: null,
    });
    mocks.listApplications.mockResolvedValue([]);
    mocks.listFieldDefinitions.mockResolvedValue([]);
    mocks.listApplicationViews.mockResolvedValue([]);
    mocks.getApplicationPageSize.mockResolvedValue(50);
    mocks.listUnlinkedFolders.mockResolvedValue([]);
    mocks.listWorkflowTemplates.mockResolvedValue([workflowFixture()]);
    mocks.getWorkflowTemplate.mockResolvedValue(workflowFixture());
  });

  it("starts on the overview without invented business data", async () => {
    render(<App />);

    expect(
      await screen.findByRole("heading", { name: "概览" }),
    ).toBeInTheDocument();
    expect(screen.getByText("先打开或新建本地数据仓库")).toBeInTheDocument();
    expect(screen.queryByText(/投递总数/)).not.toBeInTheDocument();
  });

  it("opens independent and contextual help without navigating away from an unsaved form", async () => {
    mocks.openHelp.mockResolvedValue(undefined);
    mocks.listApplications.mockResolvedValue([applicationFixture()]);
    mocks.getApplication.mockResolvedValue(applicationFixture());
    mocks.getStartupState.mockResolvedValue({
      rememberedWarehousePath: null,
      activeWarehouse: {
        warehouseId: "help-test",
        formatVersion: 1,
        displayPath: "synthetic-help-directory",
        accessMode: "write",
        warnings: [],
      },
    });
    render(<App />);
    await screen.findByText("synthetic-help-directory");
    fireEvent.click(screen.getByRole("button", { name: /投递记录/ }));
    fireEvent.click(await screen.findByText("示例公司"));
    const company = await screen.findByLabelText("岗位名称");
    fireEvent.change(company, { target: { value: "未保存的公司" } });
    fireEvent.keyDown(window, { key: "F1" });
    await waitFor(() => expect(mocks.openHelp).toHaveBeenCalledWith());
    expect(company).toHaveValue("未保存的公司");
    expect(screen.queryByText("有未保存的修改")).not.toBeInTheDocument();
    // Contextual help also must not run draft navigation logic.
    fireEvent.click(screen.getByRole("button", { name: "本页帮助" }));
    await waitFor(() =>
      expect(mocks.openHelp).toHaveBeenCalledWith("applications"),
    );
    expect(company).toHaveValue("未保存的公司");
    mocks.openHelp.mockRejectedValue(new Error("runtime error"));
    fireEvent.keyDown(window, { key: "F1" });
    await screen.findByText(/帮助窗口未打开，请重试/);
    expect(company).toHaveValue("未保存的公司");
  });

  it("offers offline full-backup recovery even when no warehouse can be opened", async () => {
    render(<App />);
    await screen.findByText("先打开或新建本地数据仓库");
    fireEvent.click(screen.getByRole("button", { name: "备份与迁移" }));
    expect(
      await screen.findByRole("heading", { name: "完整备份与迁移" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "选择并校验完整备份" }),
    ).toBeEnabled();
    expect(screen.getByRole("button", { name: "创建完整备份" })).toBeDisabled();
  });

  it("keeps the source on failed migration switch and resets context after switching to the same ID at a new path", async () => {
    const source = {
      warehouseId: "same-id",
      formatVersion: 1,
      displayPath: "source-directory",
      accessMode: "write",
      warnings: [],
    };
    mocks.getStartupState.mockResolvedValue({
      rememberedWarehousePath: null,
      activeWarehouse: source,
    });
    mocks.selectDirectory.mockResolvedValue("destination");
    mocks.migrateWarehouse.mockResolvedValue({
      directory: "verified-directory",
      warehouseId: source.warehouseId,
      applicationCount: 0,
      documentCount: 0,
      includesRecycleBin: true,
      migrationBackupPath: "migration.offertrack-backup",
    });
    mocks.openWarehouse
      .mockRejectedValueOnce(new Error("cannot open"))
      .mockResolvedValueOnce({ ...source, displayPath: "verified-directory" });
    render(<App />);
    await screen.findByText("source-directory");
    fireEvent.click(screen.getByRole("button", { name: "备份与迁移" }));
    const migrateButton = await screen.findByRole("button", {
      name: "迁移当前仓库…",
    });
    // Resolve the async directory picker and commit the dialog's showModal effect
    // before querying visible buttons; a mounted but closed dialog is not interactive.
    await act(async () => {
      fireEvent.click(migrateButton);
      await Promise.resolve();
    });
    fireEvent.click(
      await screen.findByRole("button", { name: "开始迁移复制" }),
    );
    fireEvent.click(
      await screen.findByRole("button", { name: "切换到已验证仓库" }),
    );
    await waitFor(() => expect(mocks.openWarehouse).toHaveBeenCalledTimes(1));
    expect(
      screen.getByText(/迁移完整备份：migration.offertrack-backup/),
    ).toBeInTheDocument();
    await waitFor(() =>
      expect(
        screen.getByRole("button", { name: "切换到已验证仓库" }),
      ).toBeEnabled(),
    );
    fireEvent.click(screen.getByRole("button", { name: "切换到已验证仓库" }));
    await screen.findByText("已切换到验证后的仓库；原仓库和完整备份保持不动。");
    expect(
      screen.queryByRole("button", { name: "切换到已验证仓库" }),
    ).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "概览" }));
    await screen.findByText("verified-directory");
  });

  it("opens the phase two application workspace for an active warehouse", async () => {
    mocks.getStartupState.mockResolvedValue({
      rememberedWarehousePath: null,
      activeWarehouse: {
        warehouseId: "45b1e0ad-74d4-4b8a-978c-8b9d557ad613",
        formatVersion: 1,
        displayPath: "D:\\OfferTrack-data",
        accessMode: "write",
        warnings: [],
      },
    });
    render(<App />);
    await screen.findByRole("heading", { name: "概览" });
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "打开仓库" })).toBeEnabled(),
    );

    fireEvent.click(screen.getByRole("button", { name: /投递记录/ }));

    await screen.findByPlaceholderText("搜索公司、岗位、标签或备注");
    expect(
      await screen.findByRole("button", { name: "新建投递" }),
    ).toBeInTheDocument();
    expect(
      screen.getByPlaceholderText("搜索公司、岗位、标签或备注"),
    ).toBeInTheDocument();
    await waitFor(() =>
      expect(mocks.listApplications).toHaveBeenCalledWith("active"),
    );
  });

  it("creates a selected warehouse and displays the backend summary", async () => {
    mocks.selectDirectory.mockResolvedValue("D:\\OfferTrack-data");
    mocks.createWarehouse.mockResolvedValue({
      warehouseId: "45b1e0ad-74d4-4b8a-978c-8b9d557ad613",
      formatVersion: 1,
      displayPath: "D:\\OfferTrack-data",
      accessMode: "write",
      warnings: [],
    });
    render(<App />);
    await screen.findByRole("heading", { name: "概览" });
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "打开仓库" })).toBeEnabled(),
    );

    fireEvent.click(screen.getByRole("button", { name: "新建数据仓库" }));

    await waitFor(() =>
      expect(mocks.createWarehouse).toHaveBeenCalledWith("D:\\OfferTrack-data"),
    );
    expect(await screen.findByText("已安全连接")).toBeInTheDocument();
    expect(screen.getByText("独占写入")).toBeInTheDocument();
  });

  it("opens templates from the sidebar and protects the draft on navigation", async () => {
    mocks.getStartupState.mockResolvedValue({
      rememberedWarehousePath: null,
      activeWarehouse: {
        warehouseId: "test-warehouse",
        formatVersion: 1,
        displayPath: "test warehouse",
        accessMode: "write",
        warnings: [],
      },
    });
    render(<App />);
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "打开仓库" })).toBeEnabled(),
    );
    fireEvent.click(screen.getByRole("button", { name: /流程模板/ }));
    fireEvent.change(await screen.findByLabelText("模板名称"), {
      target: { value: "未保存模板" },
    });
    fireEvent.click(screen.getByRole("button", { name: "概览" }));
    fireEvent.click(await screen.findByRole("button", { name: "继续编辑" }));
    expect(screen.getByLabelText("模板名称")).toHaveValue("未保存模板");
    fireEvent.click(screen.getByRole("button", { name: "概览" }));
    fireEvent.click(
      await screen.findByRole("button", { name: "放弃修改并继续" }),
    );
    await waitFor(() =>
      expect(screen.queryByLabelText("模板名称")).not.toBeInTheDocument(),
    );
  });

  it("guards page navigation and warehouse closing with an unsaved detail", async () => {
    mocks.getStartupState.mockResolvedValue({
      rememberedWarehousePath: null,
      activeWarehouse: {
        warehouseId: "test-warehouse",
        formatVersion: 1,
        displayPath: "test warehouse",
        accessMode: "write",
        warnings: [],
      },
    });
    mocks.listApplications.mockResolvedValue([applicationFixture()]);
    mocks.getApplication.mockResolvedValue(applicationFixture());
    mocks.closeWarehouse.mockResolvedValue(undefined);
    render(<App />);
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "打开仓库" })).toBeEnabled(),
    );
    fireEvent.click(screen.getByRole("button", { name: /投递记录/ }));
    fireEvent.click(await screen.findByText("示例公司"));
    fireEvent.change(await screen.findByLabelText("岗位名称"), {
      target: { value: "未保存的岗位" },
    });
    fireEvent.click(screen.getByRole("button", { name: "概览" }));
    fireEvent.click(await screen.findByText("继续编辑"));
    expect(screen.getByLabelText("岗位名称")).toHaveValue("未保存的岗位");
    fireEvent.click(screen.getByRole("button", { name: "关闭仓库" }));
    fireEvent.click(await screen.findByText("继续编辑"));
    expect(mocks.closeWarehouse).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "关闭仓库" }));
    fireEvent.click(await screen.findByText("放弃修改并继续"));
    await waitFor(() => expect(mocks.closeWarehouse).toHaveBeenCalledOnce());
    expect(
      await screen.findByText("先打开或新建本地数据仓库"),
    ).toBeInTheDocument();
  });

  it.each([
    "WAREHOUSE_LOCKED",
    "COPY_RECOVERY_REQUIRED",
    "FILE_BUSY",
    "FILE_TYPE_MISMATCH",
    "UNSAFE_PATH_REJECTED",
  ])(
    "offers a read-only way to inspect data when opening fails with %s",
    async (code) => {
      const path = "D:\\OfferTrack-data";
      mocks.selectDirectory.mockResolvedValue(path);
      mocks.openWarehouse
        .mockRejectedValueOnce(
          new OfferTrackError({
            code,
            message: "写入暂不可用",
            retryable: true,
          }),
        )
        .mockResolvedValue({
          warehouseId: "45b1e0ad-74d4-4b8a-978c-8b9d557ad613",
          formatVersion: 1,
          displayPath: path,
          accessMode: "readOnly",
          warnings: [],
        });
      render(<App />);
      await screen.findByRole("heading", { name: "概览" });
      await waitFor(() =>
        expect(screen.getByRole("button", { name: "打开仓库" })).toBeEnabled(),
      );
      fireEvent.click(screen.getByRole("button", { name: "打开已有仓库" }));
      fireEvent.click(await screen.findByRole("button", { name: "只读打开" }));
      await waitFor(() =>
        expect(mocks.openWarehouse).toHaveBeenLastCalledWith(path, "readOnly"),
      );
      expect(await screen.findByText("只读")).toBeInTheDocument();
    },
  );
});
