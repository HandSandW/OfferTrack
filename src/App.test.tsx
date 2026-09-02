import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { App } from "./App";

const mocks = vi.hoisted(() => ({
  getStartupState: vi.fn(),
  createWarehouse: vi.fn(),
  openWarehouse: vi.fn(),
  closeWarehouse: vi.fn(),
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
  });

  it("starts on the overview without invented business data", async () => {
    render(<App />);

    expect(
      await screen.findByRole("heading", { name: "概览" }),
    ).toBeInTheDocument();
    expect(
      screen.getByText("选择一个本地文件夹作为数据仓库"),
    ).toBeInTheDocument();
    expect(screen.queryByText(/投递总数/)).not.toBeInTheDocument();
  });

  it("keeps later-phase pages as explicit placeholders", async () => {
    render(<App />);
    await screen.findByRole("heading", { name: "概览" });

    fireEvent.click(screen.getByRole("button", { name: /投递记录/ }));

    expect(
      screen.getByRole("heading", { name: "投递记录", level: 2 }),
    ).toBeInTheDocument();
    expect(screen.getByText(/计划在阶段 2 实现/)).toBeInTheDocument();
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

    fireEvent.click(screen.getByRole("button", { name: "新建数据仓库" }));

    await waitFor(() =>
      expect(mocks.createWarehouse).toHaveBeenCalledWith("D:\\OfferTrack-data"),
    );
    expect(await screen.findByText("已安全连接")).toBeInTheDocument();
    expect(screen.getByText("独占写入")).toBeInTheDocument();
  });
});
