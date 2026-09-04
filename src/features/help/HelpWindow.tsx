import { useEffect, useReducer, useRef, useState } from "react";
import { isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { helpApi, type HelpLocation } from "./api";
import { chapters, mitLicense } from "./content";
import {
  readingHistory,
  resolveHelpLink,
  searchChapters,
  topicChapter,
} from "./model";
import { Markdown } from "./Markdown";
import { Diagnostics } from "./Diagnostics";

export function HelpWindow() {
  const [history, dispatch] = useReducer(readingHistory, {
    entries: [topicChapter("manual")],
    index: 0,
  });
  const [query, setQuery] = useState("");
  const [error, setError] = useState("");
  const search = useRef<HTMLInputElement>(null);
  const article = useRef<HTMLElement>(null);
  const title = useRef<HTMLHeadingElement>(null);
  const chapter =
    chapters.find((item) => item.id === history.entries[history.index]) ??
    chapters[0]!;
  const results = searchChapters(chapters, query);
  useEffect(() => {
    article.current?.scrollTo?.(0, 0);
    title.current?.focus({ preventScroll: true });
  }, [chapter.id]);
  useEffect(() => {
    const keydown = (event: KeyboardEvent) => {
      if (event.key === "F1") {
        event.preventDefault();
        dispatch({ chapter: topicChapter("manual") });
      } else if (event.ctrlKey && event.key.toLowerCase() === "f") {
        event.preventDefault();
        search.current?.focus();
        search.current?.select();
      } else if (
        event.altKey &&
        ["ArrowLeft", "ArrowRight"].includes(event.key)
      ) {
        event.preventDefault();
        dispatch({ step: event.key === "ArrowLeft" ? -1 : 1 });
      }
    };
    window.addEventListener("keydown", keydown);
    return () => window.removeEventListener("keydown", keydown);
  }, []);
  useEffect(() => {
    if (!isTauri()) return;
    let disposed = false;
    let revision = -1;
    let stop: (() => void) | undefined;
    const receive = (location: HelpLocation) => {
      if (!disposed && location.revision > revision) {
        revision = location.revision;
        setQuery("");
        dispatch({ chapter: topicChapter(location.topic) });
      }
    };
    // Subscribe before reading the retained target: no lost cold-start/menu race.
    void listen<HelpLocation>("help-location", (event) =>
      receive(event.payload),
    )
      .then(async (unlisten) => {
        if (disposed) {
          unlisten();
          return;
        }
        stop = unlisten;
        receive(await helpApi.location());
      })
      .catch(() => {
        if (!disposed)
          setError(
            "章节定位连接失败，仍可用左侧目录阅读完整离线帮助；请关闭帮助后重开。",
          );
      });
    return () => {
      disposed = true;
      stop?.();
    };
  }, []);
  const navigate = (href: string) => {
    const target = resolveHelpLink(href, chapter, chapters);
    if (target) dispatch({ chapter: target });
  };
  return (
    <div className="help-shell">
      <header className="help-header">
        <div className="help-brand">
          <img src="/app-icon.png" alt="" aria-hidden="true" />
          <div>
            <p className="eyebrow">OfferTrack · 离线使用手册</p>
            <h1>使用帮助</h1>
          </div>
        </div>
        <div className="help-actions">
          <button
            type="button"
            disabled={history.index === 0}
            onClick={() => dispatch({ step: -1 })}
          >
            后退
          </button>
          <button
            type="button"
            disabled={history.index === history.entries.length - 1}
            onClick={() => dispatch({ step: 1 })}
          >
            前进
          </button>
          <button
            type="button"
            onClick={() => dispatch({ chapter: topicChapter("manual") })}
          >
            快速开始
          </button>
        </div>
      </header>
      <aside className="help-toc" aria-label="帮助目录">
        <label>
          搜索手册
          <input
            ref={search}
            type="search"
            maxLength={200}
            value={query}
            placeholder="例如：恢复 简历"
            onChange={(event) => setQuery(event.target.value)}
          />
        </label>
        <p className="muted" role="status">
          {query.trim()
            ? `找到 ${results.length} 个章节`
            : `${results.length} 个章节 · 无需联网`}
        </p>
        <nav aria-label="手册章节">
          {results.map((item) => (
            <button
              type="button"
              key={item.id}
              aria-label={item.title}
              aria-current={item.id === chapter.id ? "page" : undefined}
              onClick={() => dispatch({ chapter: item.id })}
            >
              <span>{item.title}</span>
              <small>
                {item.source === "guide"
                  ? "使用指南"
                  : item.source === "agent"
                    ? "Agent 接口附录"
                    : "备份格式附录"}
              </small>
              {query.trim() && (
                <span className="help-excerpt">{item.body.slice(0, 100)}…</span>
              )}
            </button>
          ))}
        </nav>
        {results.length === 0 && (
          <p>没有匹配章节，试试更短的关键词或清空搜索。</p>
        )}
      </aside>
      <main className="help-article" ref={article}>
        {error && <p role="alert">{error}</p>}
        <h2 tabIndex={-1} ref={title}>
          {chapter.title}
        </h2>
        <Markdown
          body={chapter.body}
          navigate={navigate}
          resolve={(href) => !!resolveHelpLink(href, chapter, chapters)}
        />
        {chapter.id === topicChapter("diagnostics") && (
          <Diagnostics key={chapter.id} />
        )}
        {chapter.id === topicChapter("about") && (
          <pre className="help-license">{mitLicense}</pre>
        )}
      </main>
    </div>
  );
}
