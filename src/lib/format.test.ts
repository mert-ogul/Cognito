import { describe, expect, it } from "vitest";
import { fileName, formatModelBytes, pageLabel } from "./format";

describe("fileName", () => {
  it("takes the last path segment for unix and windows separators", () => {
    expect(fileName("/vault/a/report.pdf")).toBe("report.pdf");
    expect(fileName("C:\\vault\\a\\report.pdf")).toBe("report.pdf");
    expect(fileName("report.pdf")).toBe("report.pdf");
  });
});

describe("pageLabel", () => {
  it("formats single and multi-page ranges", () => {
    expect(pageLabel(3, 3)).toBe("p. 3");
    expect(pageLabel(3, 5)).toBe("pp. 3-5");
    expect(pageLabel(3, null)).toBe("p. 3");
    expect(pageLabel(null, 5)).toBeNull();
  });
});

describe("formatModelBytes", () => {
  it("formats MB and GB", () => {
    expect(formatModelBytes(0)).toBe("0 MB");
    expect(formatModelBytes(1024 * 1024)).toBe("1 MB");
    expect(formatModelBytes(1024 * 1024 * 1024)).toBe("1.0 GB");
  });
});
