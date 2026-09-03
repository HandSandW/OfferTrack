import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import type { InstalledBrowser } from "../contracts";
import { desktopApi } from "../lib/tauri";

export interface MenuAction {
  label: string;
  run: () => void;
}
export function OpenMenu({
  x,
  y,
  actions,
  openInBrowser,
  onClose,
}: {
  x: number;
  y: number;
  actions: MenuAction[];
  openInBrowser?: ((browser: InstalledBrowser) => void) | undefined;
  onClose: () => void;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const [browsers, setBrowsers] = useState<InstalledBrowser[]>([]);
  const [browserState, setBrowserState] = useState("正在检测浏览器…");
  useEffect(() => {
    if (!openInBrowser) return;
    let active = true;
    void desktopApi
      .availableBrowsers()
      .then((items) => {
        if (active) {
          setBrowsers(items);
          setBrowserState(items.length ? "" : "未在常见安装位置检测到浏览器");
        }
      })
      .catch(() => {
        if (active) setBrowserState("浏览器检测失败；仍可用默认方式打开");
      });
    return () => {
      active = false;
    };
  }, [openInBrowser]);
  useLayoutEffect(() => {
    const menu = ref.current;
    if (!menu) return;
    const previous = document.activeElement;
    menu.style.left = `${Math.max(8, Math.min(x, window.innerWidth - menu.offsetWidth - 8))}px`;
    menu.style.top = `${Math.max(8, Math.min(y, window.innerHeight - menu.offsetHeight - 8))}px`;
    menu.querySelector<HTMLButtonElement>("button")?.focus();
    return () => {
      if (previous instanceof HTMLElement && previous.isConnected)
        previous.focus();
    };
  }, [x, y]);
  useEffect(() => {
    const outside = (event: PointerEvent) => {
      if (!ref.current?.contains(event.target as Node)) onClose();
    };
    document.addEventListener("pointerdown", outside);
    return () => document.removeEventListener("pointerdown", outside);
  }, [onClose]);
  const labels = { edge: "Edge", chrome: "Chrome", firefox: "Firefox" };
  const choices = [
    ...actions,
    ...browsers.map((browser) => ({
      label: `使用 ${labels[browser]} 打开`,
      run: () => openInBrowser?.(browser),
    })),
  ];
  return createPortal(
    <div
      ref={ref}
      className="open-menu"
      role="menu"
      aria-label="打开方式"
      onClick={(event) => event.stopPropagation()}
      onContextMenu={(event) => event.preventDefault()}
      onKeyDown={(event) => {
        if (event.key === "Escape" || event.key === "Tab") {
          event.preventDefault();
          event.stopPropagation();
          onClose();
          return;
        }
        if (!["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key))
          return;
        event.preventDefault();
        const buttons = Array.from(
          ref.current?.querySelectorAll<HTMLButtonElement>("button") ?? [],
        );
        const index = buttons.indexOf(
          document.activeElement as HTMLButtonElement,
        );
        const next =
          event.key === "Home"
            ? 0
            : event.key === "End"
              ? buttons.length - 1
              : (index +
                  (event.key === "ArrowDown" ? 1 : -1) +
                  buttons.length) %
                buttons.length;
        buttons[next]?.focus();
      }}
    >
      {choices.map((action) => (
        <button
          key={action.label}
          type="button"
          role="menuitem"
          onClick={() => {
            onClose();
            action.run();
          }}
        >
          {action.label}
        </button>
      ))}
      {openInBrowser && browserState && (
        <small role="status">{browserState}</small>
      )}
    </div>,
    document.body,
  );
}
