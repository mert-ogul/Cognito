import { expect, test } from "@playwright/test";

test("shell renders toolbar and panes", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByText("Cognito", { exact: true })).toBeVisible();
  await expect(page.getByText("Library", { exact: true })).toBeVisible();
  await expect(page.getByLabel("Chat message")).toBeVisible();
  await expect(page.getByRole("button", { name: "Connections" }).first()).toBeVisible();
  await expect(page.getByRole("button", { name: "Open settings" })).toBeVisible();
});

test.describe("E2E mock build", () => {
  test("fixture library, chat stream, citations, connections", async ({ page }) => {
    await page.goto("/");
    await expect(page.getByText("Fixture sample")).toBeVisible();

    const toolbarConnections = page
      .getByRole("banner")
      .getByRole("button", { name: "Connections" });
    if (!(await page.getByRole("button", { name: "Passage" }).isVisible())) {
      await toolbarConnections.click();
    }
    await expect(page.getByRole("button", { name: "Passage" })).toBeVisible();

    const sideConnections = page
      .locator("aside")
      .last()
      .getByRole("button", { name: "Connections" });
    await sideConnections.click();
    await expect(page.getByRole("button", { name: "Index" })).toBeVisible();

    const input = page.getByRole("textbox", { name: "Chat message" });
    await input.fill("What is in the library?");
    await input.press("Enter");
    await expect(page.getByText(/Hello world/)).toBeVisible({ timeout: 15_000 });
    const cite = page.getByRole("button", { name: "Fixture sample", exact: true });
    await expect(cite).toBeVisible();
    await cite.click();
    await expect(page.getByRole("button", { name: "Passage", pressed: true })).toBeVisible();
  });
});
