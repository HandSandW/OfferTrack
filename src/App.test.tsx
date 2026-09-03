import {
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

const mocks = vi.hoisted(() => ({
  getStartupState: vi.fn(),
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
  selectDirectory: vi.fn(),
}));

vi.mock("./lib/tauri", async () => {
  const actual =
    await vi.importActual<typeof import("./lib/tauri")>("./lib/tauri");
  return {
    ...actual,
    desktopApi: {
      getStartupState: mocks.getStartupState,
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
    },
  };
});

vi.mock("./lib/dialog", () => ({ selectDirectory: mocks.selectDirectory }));

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn().mockResolvedValue(() => undefined),
}));

describe("OfferTrack app shell", () => {
  afterEach(() => cleanup());

  beforeEach(() => {
    vi.clearAllMocks();
    mocks.getStartupState.mockResolvedValue({
      rememberedWarehousePath: null,
      activeWarehouse: null,
    });
    mocks.listApplications.mockResolvedValue([]);
    mocks.listFieldDefinitions.mockResolvedValue([]);
    mocks.listApplicationViews.mockResolvedValue([]);
    mocks.getApplicationPageSize.mockResolvedValue(50);
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
    fireEvent.click(screen.getByRole("button", { name: "帮助" }));
    fireEvent.click(await screen.findByRole("button", { name: "继续编辑" }));
    expect(screen.getByLabelText("模板名称")).toHaveValue("未保存模板");
    fireEvent.click(screen.getByRole("button", { name: "帮助" }));
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
    fireEvent.click(screen.getByRole("button", { name: "帮助" }));
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
