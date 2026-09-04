import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { listen } from "@tauri-apps/api/event";
import { afterEach, expect, it, vi } from "vitest";
import { desktopApi } from "../../lib/tauri";
import { DraftGuardProvider } from "../../shared/DraftGuardProvider";
import { applicationFixture } from "../../test/applicationFixture";
import { ApplicationDetailWindow } from "./ApplicationDetailWindow";

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn().mockResolvedValue(() => undefined),
}));

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  vi.clearAllMocks();
});

it("renders a visible loading state before the detail data is ready", async () => {
  let resolveDetail!: (value: ReturnType<typeof applicationFixture>) => void;
  vi.spyOn(desktopApi, "getApplicationDetailTarget").mockResolvedValue({
    applicationId: "application-one",
    revision: 1,
  });
  vi.spyOn(desktopApi, "getApplication").mockReturnValue(
    new Promise((resolve) => {
      resolveDetail = resolve;
    }),
  );
  vi.spyOn(desktopApi, "listFieldDefinitions").mockResolvedValue([]);
  vi.spyOn(desktopApi, "getStartupState").mockResolvedValue({
    rememberedWarehousePath: null,
    activeWarehouse: {
      warehouseId: "warehouse-one",
      displayPath: "synthetic",
      formatVersion: 1,
      accessMode: "write",
      warnings: [],
    },
  });
  render(<ApplicationDetailWindow />, { wrapper: DraftGuardProvider });
  expect(screen.getByText("正在读取投递详情…")).toBeInTheDocument();

  resolveDetail(applicationFixture({ id: "application-one" }));
  expect(await screen.findByText("示例公司")).toBeInTheDocument();
});

it("renders an actionable error page in the visible detail window", async () => {
  vi.spyOn(desktopApi, "getApplicationDetailTarget").mockRejectedValue(
    new Error("synthetic load failure"),
  );
  render(<ApplicationDetailWindow />, { wrapper: DraftGuardProvider });
  expect(await screen.findByRole("alert")).toHaveTextContent(
    "synthetic load failure",
  );
});

it("keeps the selected tab while a focused application replaces the detail", async () => {
  const first = applicationFixture({
    id: "application-one",
    companyName: "第一家公司",
    folderRelativePath: "applications/one",
  });
  const second = applicationFixture({
    id: "application-two",
    companyName: "第二家公司",
    folderRelativePath: "applications/two",
  });
  vi.spyOn(desktopApi, "getApplicationDetailTarget").mockResolvedValue({
    applicationId: first.id,
    revision: 1,
  });
  vi.spyOn(desktopApi, "getApplication").mockImplementation((id) =>
    Promise.resolve(id === first.id ? first : second),
  );
  vi.spyOn(desktopApi, "listFieldDefinitions").mockResolvedValue([]);
  vi.spyOn(desktopApi, "getStartupState").mockResolvedValue({
    rememberedWarehousePath: null,
    activeWarehouse: {
      warehouseId: "warehouse-one",
      displayPath: "synthetic",
      formatVersion: 1,
      accessMode: "write",
      warnings: [],
    },
  });
  const inspect = vi
    .spyOn(desktopApi, "inspectApplicationFiles")
    .mockImplementation((applicationId) =>
      Promise.resolve({
        state: "available",
        relativePath:
          applicationId === first.id
            ? first.folderRelativePath
            : second.folderRelativePath,
      }),
    );
  vi.spyOn(desktopApi, "listApplicationDirectories").mockResolvedValue({
    version: 1,
    directories: [],
  });

  render(<ApplicationDetailWindow />, { wrapper: DraftGuardProvider });
  expect(await screen.findByText("第一家公司")).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "文件" }));
  await waitFor(() => expect(inspect).toHaveBeenCalledWith(first.id));

  const targetListener = vi
    .mocked(listen)
    .mock.calls.find(([event]) => event === "application-detail-target")?.[1];
  expect(targetListener).toBeDefined();
  act(() => {
    targetListener?.({
      event: "application-detail-target",
      id: 1,
      payload: { applicationId: second.id, revision: 2 },
    });
  });

  expect(await screen.findByText("第二家公司")).toBeInTheDocument();
  await waitFor(() => expect(inspect).toHaveBeenCalledWith(second.id));
  expect(screen.getByRole("button", { name: "文件" })).toHaveClass("active");
  expect(screen.queryByLabelText("公司名称")).not.toBeInTheDocument();
});
