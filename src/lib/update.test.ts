import { describe, expect, it } from "vitest";
import {
  DAY_MS,
  defaultUpdateState,
  due,
  isNewer,
  normalizeUpdateState,
  parseVersion,
  releaseUrl,
  statusFor,
} from "./update";

describe("versions", () => {
  it("compares by their numbers, not as text", () => {
    expect(parseVersion("v1.10.0")).toEqual([1, 10, 0]);
    expect(isNewer("1.10.0", "1.9.1")).toBe(true);
    expect(isNewer("2.0.0", "1.99.99")).toBe(true);
    expect(isNewer("1.1.0", "1.1.0")).toBe(false);
    expect(isNewer("1.0.9", "1.1.0")).toBe(false);
  });

  it("treats anything but major.minor.patch as unreadable, and unreadable as not newer", () => {
    for (const odd of ["1.10", "1.10.0.1", "1.10.0-rc1", "models-1", "", "v", "1..0", "1.x.0"]) {
      expect(parseVersion(odd), odd).toBeNull();
      expect(isNewer(odd, "1.0.0"), odd).toBe(false);
    }
  });

  it("links to the release page", () => {
    expect(releaseUrl("1.2.0")).toBe("https://github.com/fly2nbc-oss/VoxMD/releases/tag/v1.2.0");
    expect(releaseUrl("v1.2.0")).toBe(releaseUrl("1.2.0"));
  });
});

describe("due", () => {
  it("never asks while switched off", () => {
    expect(due(false, null, 1_000)).toBe(false);
  });

  it("asks at once when it never has, then once a day", () => {
    expect(due(true, null, 1_000)).toBe(true);
    expect(due(true, 1_000, 1_000 + DAY_MS - 1)).toBe(false);
    expect(due(true, 1_000, 1_000 + DAY_MS)).toBe(true);
  });

  it("counts a clock set back as due", () => {
    expect(due(true, 5_000_000, 1_000)).toBe(true);
  });
});

describe("state", () => {
  it("starts off and unasked", () => {
    expect(defaultUpdateState()).toMatchObject({ enabled: false, asked: false });
  });

  it("reads what the status was before any check this session", () => {
    const state = { ...defaultUpdateState(), latest: "1.2.0" };
    expect(statusFor(defaultUpdateState(), "1.1.0")).toBe("never");
    expect(statusFor(state, "1.1.0")).toBe("newer");
    expect(statusFor(state, "1.2.0")).toBe("current");
  });

  it("survives junk in the store file", () => {
    expect(normalizeUpdateState(null)).toEqual(defaultUpdateState());
    expect(normalizeUpdateState("x")).toEqual(defaultUpdateState());
    const odd = normalizeUpdateState({ enabled: "yes", latest: "soon", checkedMs: NaN });
    expect(odd).toEqual(defaultUpdateState());
  });

  it("counts a stored switch as an answer", () => {
    expect(normalizeUpdateState({ enabled: false })).toMatchObject({ enabled: false, asked: true });
    expect(normalizeUpdateState({ enabled: true, latest: "1.2.0", checkedMs: 5 })).toEqual({
      enabled: true,
      asked: true,
      checkedMs: 5,
      latest: "1.2.0",
      told: null,
    });
  });
});
