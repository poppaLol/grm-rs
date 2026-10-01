import { expect, test } from "@playwright/test";

const screenshotDir = "test-results/screenshots";

test.describe("flight-deck visual projection fixture", () => {
  test("renders data, schema, and audit views with projection hints", async ({ page }) => {
    await page.goto("/");

    await expect(page.getByRole("heading", { name: "GRM flight-deck" })).toBeVisible();
    await expect(page.getByText(/fixture snapshot:/i)).toBeVisible({ timeout: 15_000 });
    await expect(page.locator(".graph-canvas canvas").first()).toBeVisible({ timeout: 15_000 });
    const workspaceMode = page.getByLabel("Workspace mode");
    await expect(workspaceMode.getByRole("button", { name: "Data" })).toHaveClass(/active/);
    await expect(page.getByRole("complementary", { name: "Schema and projection" })).toHaveCount(0);
    await expect(page.getByLabel("Data query and filter")).toBeVisible();

    await page.screenshot({
      path: `${screenshotDir}/flight-deck-data-view.png`,
      fullPage: true
    });

    await workspaceMode.getByRole("button", { name: "Schema" }).click();
    await expect(workspaceMode.getByRole("button", { name: "Schema" })).toHaveClass(/active/);
    await expect(page.getByPlaceholder("model, field, or edge direction")).toBeVisible();
    await expect(page.locator(".schema-panel").getByText("roadmap item", { exact: true })).toBeVisible();
    await expect(page.locator(".schema-panel").getByText("work slice", { exact: true })).toBeVisible();
    await expect(page.getByRole("region", { name: "Visual projection", exact: true })).toBeVisible();
    await expect(page.getByText("Generated visual guidance; workspace data remains canonical.")).toBeVisible();
    await expect(page.getByText("local_fixture_schema_metadata")).toBeVisible();
    await expect(page.getByText("schema-by-endpoints")).toBeVisible();
    await expect(page.getByRole("region", { name: "Visual Design Space" })).toBeVisible();
    await expect(page.getByRole("region", { name: "Node Setup" })).toBeVisible();
    await expect(page.getByRole("region", { name: "Edge Links" })).toBeVisible();
    await expect(page.getByRole("region", { name: "Containers" })).toBeVisible();
    await expect(page.getByRole("region", { name: "Layout Mode" })).toBeVisible();
    await expect(page.getByRole("button", { name: "Reset to generated defaults" })).toBeVisible();
    await expect(page.locator(".graph-canvas canvas").first()).toBeVisible();

    await page.screenshot({
      path: `${screenshotDir}/flight-deck-schema-view.png`,
      fullPage: true
    });

    await workspaceMode.getByRole("button", { name: "Audit" }).click();
    await expect(workspaceMode.getByRole("button", { name: "Audit" })).toHaveClass(/active/);
    await expect(page.getByRole("heading", { name: "Audit" })).toBeVisible();
    await expect(page.getByPlaceholder("principal, workspace, outcome, operation")).toBeVisible();
    await expect(page.getByText("Service-authored audit evidence and bounded local UI observations.")).toBeVisible();
    await expect(page.getByText("bounded snapshot read")).toBeVisible();

    await page.screenshot({
      path: `${screenshotDir}/flight-deck-audit-view.png`,
      fullPage: true
    });
  });
});
