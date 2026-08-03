import { describe, expect, it } from "vitest";
import { isAudioPath, localItem } from "./queue";

describe("isAudioPath", () => {
  it("accepts known extensions case-insensitively", () => {
    expect(isAudioPath("/a/b.MP3")).toBe(true);
    expect(isAudioPath("c:\\x\\y.FlAc")).toBe(true);
    expect(isAudioPath("file.webm")).toBe(true);
  });

  it("rejects non-audio and extension-less paths", () => {
    expect(isAudioPath("/a/b.txt")).toBe(false);
    expect(isAudioPath("/a/b")).toBe(false);
    expect(isAudioPath("archive.tar.gz")).toBe(false);
  });
});

describe("localItem", () => {
  it("uses the basename for both POSIX and Windows separators", () => {
    expect(localItem("/home/me/Talk.mp3").displayName).toBe("Talk.mp3");
    expect(localItem("C:\\Users\\me\\Talk.mp3").displayName).toBe("Talk.mp3");
    expect(localItem("/home/me/Talk.mp3").kind).toBe("local");
    expect(localItem("/home/me/Talk.mp3").id).toBe("/home/me/Talk.mp3");
  });
});
