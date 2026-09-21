import { describe, expect, it } from "vitest";
import { statusFilterTones } from "./RelayApp";

describe("status filter color semantics", () => {
  it("keeps All neutral while using blue for active work and green for Done", () => {
    expect(statusFilterTones.All).toBe("neutral");
    expect(statusFilterTones["In progress"]).toBe("blue");
    expect(statusFilterTones.Done).toBe("green");
  });
});
