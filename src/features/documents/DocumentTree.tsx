import { useState } from "react";
import type { ApplicationDirectories, DocumentEntry } from "../../contracts";
import { desktopApi } from "../../lib/tauri";
import { OpenMenu } from "../../shared/OpenMenu";
import { documentTree, type DocumentBranch } from "./treeModel";

export function DocumentTree({
  documents,
  applicationId,
  disabled,
  run,
  onCopied,
  directories = [],
  onRename,
  onTrash,
}: {
  documents: DocumentEntry[];
  applicationId: string;
  disabled: boolean;
  run: (operation: () => Promise<void>) => Promise<void>;
  onCopied: () => void;
  directories?: ApplicationDirectories["directories"];
  onRename?: ((document: DocumentEntry) => void) | undefined;
  onTrash?: ((document: DocumentEntry) => void) | undefined;
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
              <summary>
                {folder.name}
                {folder.empty && <span className="muted">（空目录）</span>}
              </summary>
              {renderBranch(folder)}
            </details>
          </li>
        ))}
      {[...branch.files]
        .sort((a, b) => a.displayName.localeCompare(b.displayName))
        .map((file) => (
          <li key={file.id}>
            <article
              onDoubleClick={() => {
                if (!disabled) open(file);
              }}
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
                  onKeyDown={(event) => {
                    if (event.key === "Enter" && !disabled) open(file);
                    if (
                      !disabled &&
                      (event.key === "ContextMenu" ||
                        (event.shiftKey && event.key === "F10"))
                    ) {
                      event.preventDefault();
                      const box = event.currentTarget.getBoundingClientRect();
                      setMenu({ x: box.left, y: box.bottom, file });
                    }
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
            </article>
          </li>
        ))}
    </ul>
  );
  return (
    <div className="document-list document-tree">
      <p className="muted document-tree-hint">
        双击或按 Enter 使用默认应用打开；右键或按 Shift+F10 查看完整操作。
      </p>
      {renderBranch(documentTree(documents, directories))}
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
            ...(onRename && !menu.file.missing
              ? [{ label: "重命名…", run: () => onRename(menu.file) }]
              : []),
            ...(onTrash && !menu.file.missing
              ? [{ label: "移入附件回收站…", run: () => onTrash(menu.file) }]
              : []),
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
