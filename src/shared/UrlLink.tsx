import { useState } from "react";
import { desktopApi } from "../lib/tauri";
import { OpenMenu } from "./OpenMenu";

export function UrlLink({
  value,
  onError,
  alwaysOpen = false,
}: {
  value: string;
  onError: (error: unknown) => void;
  alwaysOpen?: boolean;
}) {
  const [menu, setMenu] = useState<{ x: number; y: number } | null>(null);
  return (
    <>
      <button
        className="cell-link"
        type="button"
        disabled={!value}
        title={`${value}\nCtrl + 点击使用默认浏览器打开；右键选择打开方式`}
        onClick={(event) => {
          if (alwaysOpen || event.ctrlKey) {
            event.stopPropagation();
            void desktopApi.openWebUrl(value).catch(onError);
          }
        }}
        onContextMenu={(event) => {
          event.preventDefault();
          event.stopPropagation();
          const box = event.currentTarget.getBoundingClientRect();
          setMenu({
            x: event.clientX || box.left,
            y: event.clientY || box.bottom,
          });
        }}
      >
        {alwaysOpen ? "打开" : value}
      </button>
      {menu && (
        <OpenMenu
          {...menu}
          onClose={() => setMenu(null)}
          actions={[
            {
              label: "使用默认浏览器打开",
              run: () => {
                void desktopApi.openWebUrl(value).catch(onError);
              },
            },
            {
              label: "复制链接",
              run: () => {
                void navigator.clipboard.writeText(value).catch(onError);
              },
            },
          ]}
          openInBrowser={(browser) => {
            void desktopApi.openWebUrl(value, browser).catch(onError);
          }}
        />
      )}
    </>
  );
}
