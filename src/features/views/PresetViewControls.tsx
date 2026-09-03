import type { BusinessState, FilterState, SortRule } from "../../contracts";
import { businessStates, matchingPreset, presetViews } from "./presets";

export function PresetViewControls({
  filter,
  sort,
  scoped,
  disabled,
  onPreset,
  onState,
  onRecycle,
}: {
  filter: FilterState;
  sort: SortRule[];
  scoped: boolean;
  disabled: boolean;
  onPreset: (id: string) => void;
  onState: (state: BusinessState | undefined) => void;
  onRecycle?: (() => void) | undefined;
}) {
  return (
    <>
      <label className="preset-control">
        快捷视图
        <select
          aria-label="快捷视图"
          value={scoped ? "" : matchingPreset(filter, sort)}
          disabled={disabled}
          title="重置筛选与排序，保留列布局、分组、详情和勾选；退出概览范围。最近更新按任意字段更新时间倒序。"
          onChange={(event) =>
            event.target.value === "recycle"
              ? onRecycle?.()
              : onPreset(event.target.value)
          }
        >
          <option value="" disabled>
            自定义筛选 / 概览范围
          </option>
          {presetViews.map(({ id, name }) => (
            <option key={id} value={id}>
              {name}
            </option>
          ))}
          {onRecycle && <option value="recycle">回收站…</option>}
        </select>
      </label>
      <label className="preset-control">
        业务状态
        <select
          aria-label="业务状态筛选"
          value={filter.businessState ?? ""}
          disabled={disabled}
          onChange={(event) =>
            onState(
              businessStates.find(([key]) => key === event.target.value)?.[0],
            )
          }
        >
          <option value="">全部业务状态</option>
          {businessStates.map(([id, name]) => (
            <option key={id} value={id}>
              {name}
            </option>
          ))}
        </select>
      </label>
    </>
  );
}
