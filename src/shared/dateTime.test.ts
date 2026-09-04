import { describe, expect, it } from "vitest";
import { formatLocalDateTime, formatOptionalLocalDateTime } from "./dateTime";

describe("formatLocalDateTime", () => {
  it("renders the selected local wall clock with an explicit UTC offset", () => {
    expect(
      formatLocalDateTime("2026-09-03T23:59:30Z", {
        timeZone: "Asia/Shanghai",
      }),
    ).toBe("2026-09-04 07:59:30 UTC+08:00");
    expect(
      formatLocalDateTime("2026-09-03T23:59:30Z", {
        timeZone: "America/New_York",
      }),
    ).toBe("2026-09-03 19:59:30 UTC-04:00");
  });

  it("handles empty and invalid values explicitly", () => {
    expect(formatOptionalLocalDateTime(null)).toBe("未设置");
    expect(formatLocalDateTime("not-a-time")).toBe("时间无效");
  });
});
