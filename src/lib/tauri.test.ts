import { afterEach, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { desktopApi } from "./tauri";
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
afterEach(() => {
  vi.restoreAllMocks();
  vi.mocked(invoke).mockReset();
});
const request = {
  applicationId: "record",
  documentId: "doc",
  expectedRelativePath: "resume.pdf",
  newName: "next.pdf",
};
it("notifies overview and Agent snapshot after a successful attachment rename", async () => {
  const dispatch = vi.spyOn(window, "dispatchEvent");
  vi.mocked(invoke).mockResolvedValue({ id: "record" });
  await desktopApi.renameDocument(request);
  expect(invoke).toHaveBeenCalledWith("rename_document", { request });
  expect(dispatch.mock.calls.map(([event]) => event.type)).toEqual([
    "offertrack-data-changed",
    "offertrack-snapshot-dirty",
  ]);
});
it("still invalidates the snapshot after recoverable file-operation failure", async () => {
  const dispatch = vi.spyOn(window, "dispatchEvent");
  vi.mocked(invoke).mockRejectedValue({
    code: "DOCUMENT_RENAME_RECOVERY",
    message: "需恢复",
    retryable: true,
  });
  await expect(desktopApi.renameDocument(request)).rejects.toThrow("需恢复");
  expect(dispatch.mock.calls.map(([event]) => event.type)).toEqual([
    "offertrack-snapshot-dirty",
  ]);
});
it("directory inspection is read-only and never signals mutation", async () => {
  const dispatch = vi.spyOn(window, "dispatchEvent");
  vi.mocked(invoke).mockResolvedValue({ version: 1, directories: [] });
  await desktopApi.listApplicationDirectories("record");
  expect(dispatch).not.toHaveBeenCalled();
});
it("uses ID-scoped attachment trash commands and invalidates derived data", async () => {
  const dispatch = vi.spyOn(window, "dispatchEvent");
  vi.mocked(invoke).mockResolvedValue({ id: "record" });
  await desktopApi.trashDocument({
    applicationId: "record",
    documentId: "doc",
    expectedRelativePath: "resume.pdf",
  });
  expect(invoke).toHaveBeenCalledWith("trash_document", {
    request: {
      applicationId: "record",
      documentId: "doc",
      expectedRelativePath: "resume.pdf",
    },
  });
  expect(dispatch.mock.calls.map(([event]) => event.type)).toEqual([
    "offertrack-data-changed",
    "offertrack-snapshot-dirty",
  ]);
});
