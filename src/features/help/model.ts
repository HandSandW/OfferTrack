export interface Chapter {
  id: string;
  title: string;
  source: string;
  body: string;
}

// Only split headings outside fenced examples; keep every body character.
export function parseChapters(markdown: string, source: string): Chapter[] {
  const chapters: Chapter[] = [];
  let title = "说明";
  let body: string[] = [];
  let fenced = false;
  const flush = () => {
    if (body.length || title !== "说明")
      chapters.push({
        id: `${source}:${title}`,
        title,
        source,
        body: body.join("\n").trim(),
      });
    body = [];
  };
  for (const line of markdown.replace(/\r\n/g, "\n").split("\n")) {
    if (line.startsWith("```")) fenced = !fenced;
    const heading = !fenced && /^(?:#|##) (.+)$/.exec(line);
    if (heading) {
      flush();
      title = heading[1]!;
    } else body.push(line);
  }
  flush();
  if (new Set(chapters.map((chapter) => chapter.id)).size !== chapters.length)
    throw new Error("Duplicate offline help chapter");
  return chapters;
}

export const topicTitles: Record<string, string> = {
  manual: "快速开始",
  "quick-start": "快速开始",
  shortcuts: "快捷键",
  data: "数据与文件说明",
  faq: "常见问题",
  diagnostics: "诊断信息",
  about: "关于 OfferTrack",
  overview: "概览、待办与提醒",
  applications: "投递记录与视图",
  tasks: "概览、待办与提醒",
  templates: "流程模板管理",
  archive: "归档与回收站",
  recycle: "归档与回收站",
  settings: "数据库备份与恢复",
  files: "文件与认领",
  agent: "让本地 Agent 分析投递",
  backup: "完整备份、独立恢复与迁移",
};

export function topicChapter(topic: string) {
  return `guide:${topicTitles[topic] ?? topicTitles.manual}`;
}

export function searchChapters(chapters: Chapter[], query: string): Chapter[] {
  const terms = query.trim().toLocaleLowerCase().split(/\s+/).filter(Boolean);
  return chapters.filter((chapter) => {
    const text = `${chapter.title}\n${chapter.body}`.toLocaleLowerCase();
    return terms.every((term) => text.includes(term));
  });
}

export interface ReadingHistory {
  entries: string[];
  index: number;
}
export type ReadingAction = { chapter: string } | { step: -1 | 1 };
export function readingHistory(
  state: ReadingHistory,
  action: ReadingAction,
): ReadingHistory {
  if ("step" in action)
    return {
      ...state,
      index: Math.max(
        0,
        Math.min(state.entries.length - 1, state.index + action.step),
      ),
    };
  if (state.entries[state.index] === action.chapter) return state;
  const entries = [
    ...state.entries.slice(0, state.index + 1),
    action.chapter,
  ].slice(-200);
  return { entries, index: entries.length - 1 };
}

export function resolveHelpLink(
  href: string,
  current: Chapter,
  chapters: Chapter[],
): string | undefined {
  const [path, fragment] = href.split("#");
  const source =
    path === "../agent-api.md"
      ? "agent"
      : path === "../backup-format.md"
        ? "backup"
        : path === ""
          ? current.source
          : null;
  if (!source) return undefined;
  let title: string | undefined;
  try {
    title = fragment ? decodeURIComponent(fragment) : undefined;
  } catch {
    return undefined;
  }
  return chapters.find(
    (chapter) =>
      chapter.source === source && (!title || chapter.title === title),
  )?.id;
}
