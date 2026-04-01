import { expect, test } from "@playwright/test";

test("shell renders three panels and status bar", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByText("Cognito", { exact: true })).toBeVisible();
  await expect(page.getByRole("heading", { name: "Sources" })).toBeVisible();
  await expect(page.getByRole("heading", { name: "Chat" })).toBeVisible();
  await expect(page.getByRole("heading", { name: "Graph" })).toBeVisible();
});

test.describe("E2E mock build", () => {
  test("fixture library, chat stream, citations, graph node detail", async ({ page }) => {
    await page.goto("/");
    await expect(page.getByText("sample.md")).toBeVisible();
    const input = page.getByRole("textbox", { name: "Chat message" });
    await input.fill("What is in the library?");
    await page.getByRole("button", { name: "Send" }).click();
    await expect(page.getByText(/Hello world/)).toBeVisible({ timeout: 15_000 });
    await expect(page.getByRole("button", { name: /Fixture sample/ })).toBeVisible();
    const search = page.getByRole("textbox", { name: "Search graph nodes by name" });
    await search.fill("Sample");
    await page.getByRole("button", { name: /Sample Entity/ }).click();
    await expect(page.getByRole("heading", { name: "Node detail" })).toBeVisible();
    await expect(
      page.getByRole("dialog", { name: "Node detail" }).getByText("Sample Entity"),
    ).toBeVisible();
  });
});
