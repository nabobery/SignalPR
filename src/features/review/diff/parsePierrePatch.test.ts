import { describe, it, expect } from "vitest";
import { parsePierrePatch } from "./parsePierrePatch";
import sampleDiff from "./__fixtures__/sample-diff.txt?raw";

describe("parsePierrePatch", () => {
  it("parses a multi-file unified diff into a stable ordered list", () => {
    const files = parsePierrePatch(sampleDiff);

    expect(files).toHaveLength(4);
    expect(files[0].name).toBe("src/utils/math.ts");
    expect(files[1].name).toBe("src/utils/string.ts");
    expect(files[2].name).toBe("src/config.ts");
    expect(files[3].name).toBe("src/deprecated.ts");
  });

  it("includes rename metadata (prevName) when present", () => {
    const files = parsePierrePatch(sampleDiff);
    const renamed = files.find((f) => f.name === "src/config.ts");

    expect(renamed).toBeDefined();
    expect(renamed!.prevName).toBe("src/old-config.ts");
  });

  it("marks deleted files with type 'deleted'", () => {
    const files = parsePierrePatch(sampleDiff);
    const deleted = files.find((f) => f.name === "src/deprecated.ts");

    expect(deleted).toBeDefined();
    expect(deleted!.type).toBe("deleted");
  });

  it("marks new files with type 'new'", () => {
    const files = parsePierrePatch(sampleDiff);
    const added = files.find((f) => f.name === "src/utils/string.ts");

    expect(added).toBeDefined();
    expect(added!.type).toBe("new");
  });

  it("returns an empty array for empty input", () => {
    expect(parsePierrePatch("")).toEqual([]);
  });

  it("returns an empty array and does not throw for malformed input", () => {
    expect(parsePierrePatch("not a valid diff at all")).toEqual([]);
  });

  it("provides hunks with correct additionStart line numbers", () => {
    const files = parsePierrePatch(sampleDiff);
    const mathFile = files[0];

    expect(mathFile.hunks.length).toBeGreaterThan(0);
    expect(mathFile.hunks[0].additionStart).toBe(1);
  });
});
