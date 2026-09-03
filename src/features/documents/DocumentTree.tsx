import { useState } from "react";
import type { DocumentEntry } from "../../contracts";
import { desktopApi } from "../../lib/tauri";
import { OpenMenu } from "../../shared/OpenMenu";
import { documentTree, type DocumentBranch } from "./treeModel";

export function DocumentTree({
  documents,
  applicationId,
  disabled,
  run,
  onCopied,
}: {
  documents: DocumentEntry[];
  applicationId: string;
  disabled: boolean;
  run: (operation: () => Promise<void>) => Promise<void>;
  onCopied: () => void;
}) {
  const [menu, setMenu] = useState<{
    x: number;
    y: number;
    file: DocumentEntry;
  } | null>(null);
  const open = (
    file: DocumentEntry,
    mode?: Parameters<typeof desktopApi.openDocument>[2],
  ) =>
    void run(() =>
      mode
        ? desktopApi.openDocument(applicationId, file.id, mode)
        : desktopApi.openDocument(applicationId, file.id),
    );
  const reveal = (file: DocumentEntry) =>
    void run(() => desktopApi.revealDocument(applicationId, file.id));
  const copy = (file: DocumentEntry) =>
    void run(async () => {
      await navigator.clipboard.writeText(
        await desktopApi.getDocumentPath(applicationId, file.id),
      );
      onCopied();
    });
  const renderBranch = (branch: DocumentBranch) => (
    <ul>
      {[...branch.folders.values()]
        .sort((a, b) => a.name.localeCompare(b.name))
        .map((folder) => (
          <li key={folder.path}>
            <details open>
              <summary>{folder.name}</summary>
              {renderBranch(folder)}
            </details>
          </li>
        ))}
      {[...branch.files]
        .sort((a, b) => a.displayName.localeCompare(b.displayName))
        .map((file) => (
          <li key={file.id}>
            <article
              onContextMenu={(event) => {
                event.preventDefault();
                event.stopPropagation();
                if (disabled) return;
                setMenu({ x: event.clientX, y: event.clientY, file });
              }}
            >
              <div>
                <strong
                  tabIndex={disabled ? -1 : 0}
                  onDoubleClick={() => {
                    if (!disabled) open(file);
                  }}
                  onKeyDown={(event) => {
                    if (event.key === "Enter" && !disabled) open(file);
                  }}
                >
                  {file.displayName}
                </strong>
                <span>
                  {file.relativePath} ·{" "}
                  {file.sizeBytes === null
                    ? "未知大小"
                    : `${Math.ceil(file.sizeBytes / 1024)} KB`}
                </span>
                {file.missing && (
                  <span className="inline-warning">
                    上次扫描未找到此文件（保留索引）
                  </span>
                )}
              </div>
              <div>
                <button disabled={disabled} onClick={() => open(file)}>
                  打开
                </button>
                <button
                  disabled={disabled}
                  onClick={() => open(file, "chooseOther")}
                >
                  选择其他应用
                </button>
                <button disabled={disabled} onClick={() => reveal(file)}>
                  所在文件夹
                </button>
                <button disabled={disabled} onClick={() => copy(file)}>
                  复制路径
                </button>
                <button
                  disabled={disabled}
                  aria-label={`${file.displayName} 更多操作`}
                  onClick={(event) => {
                    const box = event.currentTarget.getBoundingClientRect();
                    setMenu({ x: box.left, y: box.bottom, file });
                  }}
                >
                  更多
                </button>
              </div>
            </article>
          </li>
        ))}
    </ul>
  );
  return (
    <div className="document-list document-tree">
      {renderBranch(documentTree(documents))}
      {menu && !disabled && (
        <OpenMenu
          x={menu.x}
          y={menu.y}
          onClose={() => setMenu(null)}
          actions={[
            { label: "使用默认应用打开", run: () => open(menu.file) },
            {
              label: "选择其他应用…",
              run: () => open(menu.file, "chooseOther"),
            },
            { label: "打开所在文件夹", run: () => reveal(menu.file) },
            { label: "复制文件路径", run: () => copy(menu.file) },
          ]}
          openInBrowser={
            /\.pdf$/i.test(menu.file.relativePath)
              ? (browser) => open(menu.file, browser)
              : undefined
          }
        />
      )}
    </div>
  );
}
