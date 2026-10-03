import { describe, expect, it } from "vitest";

import { ERROR_CODES, isErrorCode, toAppError } from "./types";

describe("IPC error mapping", () => {
  it("keeps every code the shell can report", () => {
    for (const code of ERROR_CODES) {
      expect(toAppError({ code, message: "detail" })).toEqual({ code, message: "detail" });
    }
  });

  it("treats an unrecognised rejection as an internal failure", () => {
    expect(toAppError({ code: "printer-on-fire", message: "nope" })).toEqual({
      code: "internal",
      message: "unexpected error",
    });
    expect(toAppError(undefined)).toEqual({ code: "internal", message: "unexpected error" });
  });

  it("uses the message of an Error and of a bare string rejection", () => {
    expect(toAppError(new Error("ipc unavailable"))).toEqual({
      code: "internal",
      message: "ipc unavailable",
    });
    expect(toAppError("channel closed")).toEqual({ code: "internal", message: "channel closed" });
  });

  it("recognises only the codes the shell publishes", () => {
    expect(isErrorCode("invalid-state")).toBe(true);
    expect(isErrorCode("invalid-state ")).toBe(false);
    expect(isErrorCode(7)).toBe(false);
    expect(isErrorCode(null)).toBe(false);
  });

  it("keeps the print-path codes a failure report can carry", () => {
    for (const code of [
      "server-unavailable",
      "server-untrusted",
      "server-identity-changed",
      "not-authorized",
      "printer-not-shared",
      "queue-unavailable",
    ] as const) {
      expect(isErrorCode(code)).toBe(true);
    }
  });
});
