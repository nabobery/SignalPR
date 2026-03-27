import { describe, it, expect } from "vitest";
import { parseError } from "./ipc";

describe("parseError", () => {
  it("parses structured error object", () => {
    const err = { code: "not_found", message: "PR not found" };
    expect(parseError(err)).toEqual(err);
  });

  it("parses JSON string error", () => {
    const err = JSON.stringify({
      code: "database_error",
      message: "DB failed",
    });
    const parsed = parseError(err);
    expect(parsed.code).toBe("database_error");
  });

  it("handles plain string error", () => {
    const parsed = parseError("something broke");
    expect(parsed.code).toBe("unknown");
    expect(parsed.message).toBe("something broke");
  });

  it("handles unknown types", () => {
    const parsed = parseError(42);
    expect(parsed.code).toBe("unknown");
    expect(parsed.message).toBe("42");
  });

  it("handles null", () => {
    const parsed = parseError(null);
    expect(parsed.code).toBe("unknown");
    expect(parsed.message).toBe("null");
  });

  it("handles undefined", () => {
    const parsed = parseError(undefined);
    expect(parsed.code).toBe("unknown");
    expect(parsed.message).toBe("undefined");
  });

  it("handles JSON string missing required fields", () => {
    const err = JSON.stringify({ foo: "bar" });
    const parsed = parseError(err);
    expect(parsed.code).toBe("unknown");
    expect(parsed.message).toBe(err);
  });
});
