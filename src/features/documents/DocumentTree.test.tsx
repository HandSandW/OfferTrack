import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { desktopApi } from "../../lib/tauri";
import { DocumentTree } from "./DocumentTree";
import { documentTree } from "./treeModel";

const documents = [
  {
    id: "pdf",
    relativePath: "材料/投递/resume.PDF",
    displayName: "resume.PDF",
    sizeBytes: 1024,
    mediaType: null,
    modifiedAtUtc: null,
    missing: false,
  },
  {
    id: "word",
    relativePath: "材料\\resume.docx",
    displayName: "resume.docx",
    sizeBytes: 10,
    mediaType: null,
    modifiedAtUtc: null,
    missing: true,
  },
];
afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});
it("groups nested paths without mutating IDs and keeps missing indexes", () => {
  const tree = documentTree(documents);
  const branch = tree.folders.get("材料")!;
  expect(branch.files[0]).toBe(documents[1]);
  expect(branch.folders.get("投递")?.files[0]?.id).toBe("pdf");
});
it("shows collapsible folders, default double-click and PDF browser/right-click actions", async () => {
  const open = vi.spyOn(desktopApi, "openDocument").mockResolvedValue();
  const detect = vi
    .spyOn(desktopApi, "availableBrowsers")
    .mockResolvedValue(["edge"]);
  const reveal = vi.spyOn(desktopApi, "revealDocument").mockResolvedValue();
  render(
    <DocumentTree
      applicationId="record"
      documents={documents}
      disabled={false}
      run={(operation) => operation()}
      onCopied={vi.fn()}
    />,
  );
  expect(screen.getByText("材料").closest("details")).toHaveAttribute("open");
  fireEvent.doubleClick(screen.getByText("resume.PDF"));
  expect(open).toHaveBeenCalledWith("record", "pdf");
  fireEvent.contextMenu(screen.getByText("resume.PDF"));
  fireEvent.click(
    await screen.findByRole("menuitem", { name: "使用 Edge 打开" }),
  );
  expect(open).toHaveBeenLastCalledWith("record", "pdf", "edge");
  fireEvent.click(screen.getByRole("button", { name: "resume.docx 更多操作" }));
  expect(
    screen.queryByRole("menuitem", { name: "使用 Edge 打开" }),
  ).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("menuitem", { name: "选择其他应用…" }));
  expect(open).toHaveBeenLastCalledWith("record", "word", "chooseOther");
  fireEvent.contextMenu(screen.getByText("resume.docx"));
  fireEvent.click(screen.getByRole("menuitem", { name: "打开所在文件夹" }));
  expect(reveal).toHaveBeenCalledWith("record", "word");
  expect(detect).toHaveBeenCalledTimes(1);
  await waitFor(() =>
    expect(screen.queryByRole("menu")).not.toBeInTheDocument(),
  );
});
