import type { DocumentEntry } from "../../contracts";

export interface DocumentBranch {
  name: string;
  path: string;
  folders: Map<string, DocumentBranch>;
  files: DocumentEntry[];
}
// Presentation only: never use these paths as authority for file operations.
export function documentTree(documents: DocumentEntry[]): DocumentBranch {
  const root: DocumentBranch = {
    name: "",
    path: "",
    folders: new Map(),
    files: [],
  };
  for (const document of documents) {
    const segments = document.relativePath.replaceAll("\\", "/").split("/");
    segments.pop();
    let branch = root;
    for (const name of segments) {
      if (!branch.folders.has(name))
        branch.folders.set(name, {
          name,
          path: `${branch.path}/${name}`,
          folders: new Map(),
          files: [],
        });
      branch = branch.folders.get(name)!;
    }
    branch.files.push(document);
  }
  return root;
}
