import type { ApplicationDirectories, DocumentEntry } from "../../contracts";

export interface DocumentBranch {
  name: string;
  path: string;
  folders: Map<string, DocumentBranch>;
  files: DocumentEntry[];
  empty?: boolean;
}
// Presentation only: never use these paths as authority for file operations.
export function documentTree(
  documents: DocumentEntry[],
  directories: ApplicationDirectories["directories"] = [],
): DocumentBranch {
  const root: DocumentBranch = {
    name: "",
    path: "",
    folders: new Map(),
    files: [],
  };
  const folderFor = (segments: string[]) => {
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
    return branch;
  };
  for (const directory of directories) {
    const branch = folderFor(
      directory.relativePath.replaceAll("\\", "/").split("/"),
    );
    branch.empty = directory.empty;
  }
  for (const document of documents) {
    const segments = document.relativePath.replaceAll("\\", "/").split("/");
    segments.pop();
    folderFor(segments).files.push(document);
  }
  return root;
}
