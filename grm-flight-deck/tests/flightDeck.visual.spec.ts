import { expect, type Locator, type Page, test } from "@playwright/test";

const STORE_STORAGE_KEY = "grm-flight-deck.graph-store.v1";
const VISUAL_OVERLAY_STORAGE_KEY = "grm-flight-deck.visual-overlays.v1";

const pageErrors = new WeakMap<Page, string[]>();

const SECRET_NEEDLES = [
  "alice",
  "s3cr3t",
  "token=",
  "frag",
  "BEGIN PRIVATE KEY",
  "privateKey",
  "client.key",
  "client.crt",
  "policyTable",
  "fingerprint"
];

test.describe("flight-deck Visual Design Space", () => {
  test.beforeEach(async ({ page }) => {
    collectUnexpectedBrowserErrors(page);
    await page.goto("/");
    await expect(page.getByRole("heading", { name: "GRM flight-deck" })).toBeVisible();
    await expect(page.getByText(/fixture snapshot:/i)).toBeVisible({ timeout: 15_000 });
    await expect(graphCanvas(page)).toBeVisible({ timeout: 15_000 });
  });

  test.afterEach(async ({ page }) => {
    expect(pageErrors.get(page) ?? []).toEqual([]);
  });

  test("loads the default fixture projection in Data mode", async ({ page }) => {
    const workspaceMode = page.getByLabel("Workspace mode");

    await expect(workspaceMode.getByRole("button", { name: "Data" })).toHaveClass(/active/);
    await expect(page.getByLabel("Data query and filter")).toBeVisible();
    await expect(page.getByRole("complementary", { name: "Schema and projection" })).toHaveCount(0);
    await expect(graphCanvas(page)).toBeVisible();

    await openSchemaMode(page);
    await expect(page.getByRole("region", { name: "Visual projection", exact: true })).toBeVisible();
    await expect(page.getByText("Generated visual guidance; workspace data remains canonical.")).toBeVisible();
    await expect(page.getByText("local_fixture_schema_metadata")).toBeVisible();
    await expect(page.getByText("schema-by-endpoints")).toBeVisible();
  });

  test("keeps schema filter, graph, and sidebar in sync", async ({ page }) => {
    await openSchemaMode(page);
    const panel = schemaPanel(page);
    const filter = page.getByPlaceholder("model, field, or edge direction");

    await expect(panel.getByText("roadmap item", { exact: true })).toBeVisible();
    await expect(panel.getByText("work slice", { exact: true })).toBeVisible();

    await filter.fill("HAS_WORK_SLICE");

    await expect(panel.getByText("roadmap item", { exact: true })).toBeVisible();
    await expect(panel.getByText("work slice", { exact: true })).toBeVisible();
    await expect(panel.getByText("has work slice", { exact: true })).toBeVisible();
    await expect(panel.getByText("security requirement", { exact: true })).toHaveCount(0);
    await expect(graphCanvas(page)).toBeVisible();
    await expect(page.getByRole("textbox", { name: "Schema filter" })).toBeVisible();
  });

  test("applies Node Setup overrides to local visual projection", async ({ page }) => {
    await openSchemaMode(page);
    const design = designSpace(page);
    const nodeSetup = design.getByRole("region", { name: "Node Setup" });

    await nodeSetup.getByLabel("Node model").selectOption("RoadmapItem");
    await nodeSetup.getByLabel("Display name").fill("PM roadmap");
    await nodeSetup.getByLabel("Colour").selectOption("flight-deck-amber");
    await nodeSetup.getByLabel("Glyph / shape").selectOption("card");
    await nodeSetup.getByLabel("Detail density").selectOption("rich");
    await nodeSetup.getByLabel("Visual role").selectOption("anchor");

    await expect(schemaPanel(page).getByText("PM roadmap", { exact: true })).toBeVisible();
    await expect(nodeSetup.getByLabel("Glyph / shape")).toHaveValue("card");
    await expect(nodeSetup.getByLabel("Detail density")).toHaveValue("rich");
    await expect(nodeSetup.getByLabel("Visual role")).toHaveValue("anchor");
    await expect(modelSwatchFor(schemaPanel(page), "PM roadmap")).toHaveCSS("background-color", "rgb(209, 154, 74)");
    await expect(graphCanvas(page)).toBeVisible();
  });

  test("applies Edge Links overrides including label visibility", async ({ page }) => {
    await openSchemaMode(page);
    const edgeLinks = designSpace(page).getByRole("region", { name: "Edge Links" });

    await edgeLinks.getByLabel("Edge model").selectOption("HAS_WORK_SLICE");
    await edgeLinks.getByLabel("Link label").fill("contains work");
    await edgeLinks.getByLabel("Link style").selectOption("dependency");
    await edgeLinks.getByLabel("Direction emphasis").selectOption("strong");
    await edgeLinks.getByLabel("Line weight").selectOption("strong");
    await edgeLinks.getByLabel("Line style").selectOption("dashed");
    await edgeLinks.getByLabel("Label visibility").selectOption("always");

    await expect(schemaPanel(page).getByText("contains work", { exact: true })).toBeVisible();
    await expect(edgeLinks.getByLabel("Link style")).toHaveValue("dependency");
    await expect(edgeLinks.getByLabel("Direction emphasis")).toHaveValue("strong");
    await expect(edgeLinks.getByLabel("Line weight")).toHaveValue("strong");
    await expect(edgeLinks.getByLabel("Line style")).toHaveValue("dashed");
    await expect(edgeLinks.getByLabel("Label visibility")).toHaveValue("always");
    await expect(graphCanvas(page)).toBeVisible();
  });

  test("supports advisory containers and layout mode switching", async ({ page }) => {
    await openSchemaMode(page);
    const design = designSpace(page);
    const containers = design.getByRole("region", { name: "Containers" });
    const layoutMode = design.getByRole("region", { name: "Layout Mode" });

    await expect(containers.getByText("Containment-like rendering is advisory")).toBeVisible();
    await containers.getByLabel("Containment source").selectOption("HAS_WORK_SLICE");
    await containers.getByLabel("Render as").selectOption("section");
    await containers.getByLabel("Collapse affordance").selectOption("expanded");
    await containers.getByLabel("Style token").fill("roadmap-section");

    await layoutMode.getByRole("button", { name: "Container/map" }).click();
    await layoutMode.getByLabel("Layout style").selectOption("groups");
    await expect(layoutMode.getByRole("button", { name: "Container/map" })).toHaveClass(/active/);
    await expect(graphCanvas(page)).toBeVisible();

    await layoutMode.getByRole("button", { name: "Edge-network" }).click();
    await expect(layoutMode.getByRole("button", { name: "Edge-network" })).toHaveClass(/active/);
    await expect(graphCanvas(page)).toBeVisible();
  });

  test("persists local overlays by profile and workspace only", async ({ page }) => {
    await openSchemaMode(page);
    await makeNodeAndContainerOverride(page, "PM roadmap");

    await page.reload();
    await expect(page.getByText(/fixture snapshot:/i)).toBeVisible({ timeout: 15_000 });
    await openSchemaMode(page);
    await designSpace(page).getByRole("region", { name: "Node Setup" }).getByLabel("Node model").selectOption("RoadmapItem");
    await expect(designSpace(page).getByRole("region", { name: "Node Setup" }).getByLabel("Display name")).toHaveValue("PM roadmap");
    await expect(schemaPanel(page).getByText("PM roadmap", { exact: true })).toBeVisible();
    await expect(designSpace(page).getByRole("region", { name: "Layout Mode" }).getByRole("button", { name: "Container/map" })).toHaveClass(/active/);

    await changeWorkspace(page, "another-memory");
    await openSchemaMode(page);
    await expect(schemaPanel(page).getByText("PM roadmap", { exact: true })).toHaveCount(0);
    await expect(designSpace(page).getByRole("region", { name: "Node Setup" }).getByLabel("Display name")).toHaveValue("");
    await expect(designSpace(page).getByRole("region", { name: "Layout Mode" }).getByRole("button", { name: "Edge-network" })).toHaveClass(/active/);

    await createProfile(page, "Second profile", "flight-deck-demo");
    await openSchemaMode(page);
    await expect(schemaPanel(page).getByText("PM roadmap", { exact: true })).toHaveCount(0);
  });

  test("resets node, edge, container, and layout overrides to generated defaults", async ({ page }) => {
    await openSchemaMode(page);
    await makeNodeEdgeContainerAndLayoutOverrides(page);

    await designSpace(page).getByRole("button", { name: "Reset to generated defaults" }).click();

    const design = designSpace(page);
    await expect(schemaPanel(page).getByText("PM roadmap", { exact: true })).toHaveCount(0);
    await expect(schemaPanel(page).getByText("contains work", { exact: true })).toHaveCount(0);
    await expect(design.getByRole("region", { name: "Node Setup" }).getByLabel("Display name")).toHaveValue("");
    await expect(design.getByRole("region", { name: "Node Setup" }).getByLabel("Glyph / shape")).toHaveValue("generated");
    await expect(design.getByRole("region", { name: "Edge Links" }).getByLabel("Label visibility")).toHaveValue("generated");
    await expect(design.getByRole("region", { name: "Containers" }).getByLabel("Render as")).toHaveValue("generated");
    await expect(design.getByRole("region", { name: "Layout Mode" }).getByRole("button", { name: "Edge-network" })).toHaveClass(/active/);

    const overlayPayload = await page.evaluate((key) => localStorage.getItem(key), VISUAL_OVERLAY_STORAGE_KEY);
    const overlays = overlayPayload ? JSON.parse(overlayPayload) : {};
    const current = overlays["local-workspace::flight-deck-demo"];
    expect(current === undefined || hasNoEffectiveOverrides(current)).toBeTruthy();
  });

  test("does not persist credential-bearing connection material or secret-like visual overlay data", async ({ page }) => {
    await page.getByRole("button", { name: "Change connection" }).click();
    const dialog = page.getByRole("dialog", { name: "Connection settings" });
    await dialog.getByLabel("Fixture").uncheck();
    await dialog.getByRole("textbox", { name: "Gateway URL" }).fill("https://alice:s3cr3t@127.0.0.1:3001/client.key?token=abc#frag");
    await dialog.getByRole("textbox", { name: "Profile name" }).fill("Secret scrub profile");
    await dialog.getByRole("button", { name: "Save profile" }).click();
    await dialog.getByRole("button", { name: "Close connection settings" }).click();

    await openSchemaMode(page);
    await makeNodeAndContainerOverride(page, "Secret safe roadmap");

    const storage = await page.evaluate(() => ({ ...localStorage }));
    const serializedStorage = JSON.stringify(storage);
    for (const needle of SECRET_NEEDLES) {
      expect(serializedStorage).not.toContain(needle);
    }

    const profilePayload = JSON.parse(storage[STORE_STORAGE_KEY] ?? "{}");
    expect(JSON.stringify(profilePayload)).toContain("https://127.0.0.1:3001");
    expect(JSON.stringify(profilePayload)).not.toContain("client.key");
    expect(JSON.stringify(profilePayload)).not.toContain("alice");
    expect(JSON.stringify(profilePayload)).not.toContain("s3cr3t");
    expect(JSON.stringify(profilePayload)).not.toContain("token=abc");
    expect(JSON.stringify(profilePayload)).not.toContain("#frag");

    const overlayPayload = JSON.parse(storage[VISUAL_OVERLAY_STORAGE_KEY] ?? "{}");
    expect(JSON.stringify(overlayPayload)).toContain("Secret safe roadmap");
    expect(JSON.stringify(overlayPayload)).toContain("container-map");
    for (const needle of SECRET_NEEDLES) {
      expect(JSON.stringify(overlayPayload)).not.toContain(needle);
    }
  });

  test("survives switching between Data, Schema, and Audit after design edits", async ({ page }) => {
    await openSchemaMode(page);
    await makeNodeEdgeContainerAndLayoutOverrides(page);

    const workspaceMode = page.getByLabel("Workspace mode");
    await workspaceMode.getByRole("button", { name: "Data" }).click();
    await expect(page.getByLabel("Data query and filter")).toBeVisible();
    await expect(graphCanvas(page)).toBeVisible();

    await workspaceMode.getByRole("button", { name: "Schema" }).click();
    await expect(designSpace(page)).toBeVisible();
    await expect(graphCanvas(page)).toBeVisible();

    await workspaceMode.getByRole("button", { name: "Audit" }).click();
    await expect(page.getByRole("heading", { name: "Audit" })).toBeVisible();
    await expect(page.getByPlaceholder("principal, workspace, outcome, operation")).toBeVisible();
    await expect(page.getByText("bounded snapshot read")).toBeVisible();
  });
});

function collectUnexpectedBrowserErrors(page: Page) {
  const errors: string[] = [];
  pageErrors.set(page, errors);
  page.on("console", (message) => {
    if (message.type() === "error" && !message.text().includes("Failed to load resource: the server responded with a status of 404")) {
      errors.push(message.text());
    }
  });
  page.on("pageerror", (error) => {
    errors.push(error.message);
  });
}

async function openSchemaMode(page: Page) {
  const workspaceMode = page.getByLabel("Workspace mode");
  await workspaceMode.getByRole("button", { name: "Schema" }).click();
  await expect(workspaceMode.getByRole("button", { name: "Schema" })).toHaveClass(/active/);
  await expect(schemaPanel(page)).toBeVisible();
  await expect(designSpace(page)).toBeVisible();
  await expect(graphCanvas(page)).toBeVisible();
}

function graphCanvas(page: Page): Locator {
  return page.locator(".graph-canvas canvas").first();
}

function schemaPanel(page: Page): Locator {
  return page.getByRole("complementary", { name: "Schema and projection" });
}

function designSpace(page: Page): Locator {
  return page.getByRole("region", { name: "Visual Design Space" });
}

function modelSwatchFor(panel: Locator, label: string): Locator {
  return panel.locator("li", { hasText: label }).locator(".model-swatch").first();
}

async function makeNodeAndContainerOverride(page: Page, label: string) {
  const design = designSpace(page);
  const nodeSetup = design.getByRole("region", { name: "Node Setup" });
  const containers = design.getByRole("region", { name: "Containers" });
  const layoutMode = design.getByRole("region", { name: "Layout Mode" });

  await nodeSetup.getByLabel("Node model").selectOption("RoadmapItem");
  await nodeSetup.getByLabel("Display name").fill(label);
  await nodeSetup.getByLabel("Colour").selectOption("flight-deck-amber");
  await nodeSetup.getByLabel("Glyph / shape").selectOption("card");
  await nodeSetup.getByLabel("Visual role").selectOption("anchor");
  await containers.getByLabel("Containment source").selectOption("HAS_WORK_SLICE");
  await containers.getByLabel("Render as").selectOption("section");
  await containers.getByLabel("Collapse affordance").selectOption("expanded");
  await layoutMode.getByRole("button", { name: "Container/map" }).click();
}

async function makeNodeEdgeContainerAndLayoutOverrides(page: Page) {
  await makeNodeAndContainerOverride(page, "PM roadmap");
  const edgeLinks = designSpace(page).getByRole("region", { name: "Edge Links" });
  const layoutMode = designSpace(page).getByRole("region", { name: "Layout Mode" });
  await edgeLinks.getByLabel("Edge model").selectOption("HAS_WORK_SLICE");
  await edgeLinks.getByLabel("Link label").fill("contains work");
  await edgeLinks.getByLabel("Link style").selectOption("dependency");
  await edgeLinks.getByLabel("Direction emphasis").selectOption("strong");
  await edgeLinks.getByLabel("Line weight").selectOption("strong");
  await edgeLinks.getByLabel("Line style").selectOption("dashed");
  await edgeLinks.getByLabel("Label visibility").selectOption("always");
  await layoutMode.getByLabel("Layout style").selectOption("groups");
}

async function changeWorkspace(page: Page, workspace: string) {
  await page.getByRole("button", { name: "Change connection" }).click();
  const dialog = page.getByRole("dialog", { name: "Connection settings" });
  await dialog.getByRole("textbox", { name: "Workspace" }).fill(workspace);
  await dialog.getByRole("button", { name: "Save profile" }).click();
  await dialog.getByRole("button", { name: "Connect", exact: true }).click();
  await dialog.getByRole("button", { name: "Close connection settings" }).click();
  await expect(dialog).toHaveCount(0);
  await expect(page.getByText(/fixture snapshot:/i)).toBeVisible({ timeout: 15_000 });
}

async function createProfile(page: Page, profileName: string, workspace: string) {
  await page.getByRole("button", { name: "Change connection" }).click();
  const dialog = page.getByRole("dialog", { name: "Connection settings" });
  await dialog.getByRole("textbox", { name: "Profile name" }).fill(profileName);
  await dialog.getByRole("textbox", { name: "Workspace" }).fill(workspace);
  await dialog.getByRole("button", { name: "New profile" }).click();
  await dialog.getByRole("button", { name: "Connect", exact: true }).click();
  await dialog.getByRole("button", { name: "Close connection settings" }).click();
  await expect(dialog).toHaveCount(0);
  await expect(page.getByText(/fixture snapshot:/i)).toBeVisible({ timeout: 15_000 });
}

function hasNoEffectiveOverrides(value: unknown): boolean {
  if (!value || typeof value !== "object") {
    return true;
  }
  const record = value as {
    nodeModels?: Record<string, unknown>;
    edgeModels?: Record<string, unknown>;
    containers?: Record<string, unknown>;
    layout?: Record<string, unknown>;
  };
  return Object.keys(record.nodeModels ?? {}).length === 0 &&
    Object.keys(record.edgeModels ?? {}).length === 0 &&
    Object.keys(record.containers ?? {}).length === 0 &&
    Object.keys(record.layout ?? {}).length === 0;
}
