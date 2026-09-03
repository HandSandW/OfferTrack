import { describe, expect, it } from "vitest";
import { applicationFixture } from "../../test/applicationFixture";
import { applicationDraft, localDateTime, utcDateTime } from "./editorModel";

describe("editor DTOs and local interview times", () => {
  it("normalizes tags but excludes file-index refreshes from the draft", () => {
    const detail = applicationFixture();
    expect(applicationDraft(detail, " 后端， 实习, ")).toMatchObject({
      tags: ["后端", "实习"],
      revision: 1,
    });
    expect(
      applicationDraft({
        ...detail,
        documentCount: 5,
        documentNames: ["cv.pdf"],
      }),
    ).toEqual(applicationDraft(detail));
  });
  it("round-trips a local date and preserves an untouched timestamp's precision", () => {
    const original = "2026-09-03T10:30:00.123456+08:00";
    expect(utcDateTime(localDateTime(original), original)).toBe(original);
    const local = "2026-09-04T11:45:30";
    expect(localDateTime(utcDateTime(local, original))).toBe(local);
    expect(utcDateTime("2026-09-04T11:45", null)).toBe(
      new Date("2026-09-04T11:45:00").toISOString(),
    );
    expect(utcDateTime("", original)).toBeNull();
  });
  it.each(["2026-02-30T12:00", "not-a-date", "2026-09-03", "2026-09-03T24:00"])(
    "rejects invalid local input %s",
    (value) => {
      expect(() => utcDateTime(value, null)).toThrow();
    },
  );
});
