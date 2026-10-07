import { expect, type Locator, type Page, test } from "@playwright/test";
import type { Core } from "cytoscape";
import { fixtureSecurityAuditStatus, fixtureSecurityStatus, fixtureSnapshot, fixtureVisualProjection } from "../src/fixtures";

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

    await expect(workspaceMode.getByRole("tab", { name: "Data" })).toHaveClass(/active/);
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

  test("collapses and restores the schema panel without leaving the Schema tab", async ({ page }) => {
    await openSchemaMode(page);
    await page.getByRole("button", { name: "Collapse schema panel" }).click();
    await expect(schemaPanel(page)).toHaveCount(0);
    await expect(page.getByRole("tab", { name: "Schema", exact: true })).toHaveAttribute("aria-selected", "true");
    const expand = page.getByRole("button", { name: "Expand schema panel" });
    await expect(expand).toBeVisible();
    await expect(expand.locator("svg")).toHaveCount(1);
    await expand.click();
    await expect(schemaPanel(page)).toBeVisible();
    await expect(page.getByRole("button", { name: "Collapse schema panel" }).locator("svg")).toHaveCount(1);
  });

  test("connection toggle retains its state across tabs and preserves the profile on disconnect", async ({ page }) => {
    const disconnect = page.getByRole("button", { name: "Disconnect", exact: true });
    await expect(disconnect).toHaveAttribute("aria-pressed", "true");
    await openSchemaMode(page);
    await expect(disconnect).toHaveAttribute("aria-pressed", "true");
    await disconnect.click();
    const connect = page.getByRole("button", { name: "Connect", exact: true });
    await expect(connect).toHaveAttribute("aria-pressed", "false");
    await expect(page.getByText("Not connected.", { exact: true })).toBeVisible();
    await expect(page.getByText(/fixture snapshot:/i)).toHaveCount(0);
    await expect(page.getByText("Local workspace", { exact: true })).toBeVisible();
    await connect.click();
    await expect(disconnect).toHaveAttribute("aria-pressed", "true");
    await expect(page.getByText(/fixture snapshot:/i)).toBeVisible();
  });

  for (const viewport of [{ width: 1440, height: 1000 }, { width: 390, height: 844 }]) {
    test(`hides and restores node and edge details without a collapsed rail at ${viewport.width}px`, async ({ page }) => {
      await page.setViewportSize(viewport);
      await renderedGraph(page);
      for (const id of ["13", "e2"]) {
        await page.locator(".graph-canvas").evaluate((element, itemId) => {
          const cy = (element as HTMLElement & { _cyreg: { cy: Core } })._cyreg.cy;
          cy.getElementById(itemId).emit("tap");
        }, id);
        const details = page.getByRole("complementary", { name: "Selection", exact: true });
        await expect(details).toBeVisible();
        await page.getByRole("button", { name: "Collapse selection details" }).click();
        await expect(details).toHaveCount(0);
        await expect(page.locator(".inspector.collapsed")).toHaveCount(0);
        const expand = page.getByRole("button", { name: "Open selection details" });
        await expect(expand).toBeVisible();
        await expand.click();
        await expect(details).toBeVisible();
        await page.screenshot({ path: `test-results/details-${id}-${viewport.width}.png`, fullPage: true });
        await page.getByRole("button", { name: "Collapse selection details" }).click();
      }
      await openDesignMode(page);
      await renderedGraph(page);
      await page.locator(".graph-canvas").evaluate((element) => {
        const cy = (element as HTMLElement & { _cyreg: { cy: Core } })._cyreg.cy;
        cy.getElementById("13").emit("tap");
      });
      const previewDetails = page.getByRole("complementary", { name: "Selection", exact: true });
      await expect(previewDetails).toBeVisible();
      const graphBox = await page.locator(".graph-shell").boundingBox();
      const detailsBox = await previewDetails.boundingBox();
      if (viewport.width > 1000) expect(detailsBox!.x).toBeGreaterThanOrEqual(graphBox!.x + graphBox!.width);
      else expect(detailsBox!.y).toBeGreaterThanOrEqual(graphBox!.y + graphBox!.height);
      await page.screenshot({ path: `test-results/design-details-${viewport.width}.png`, fullPage: true });
      await page.getByRole("button", { name: "Collapse selection details" }).click();
      await expect(previewDetails).toHaveCount(0);
      await page.getByRole("button", { name: "Open selection details" }).click();
      await expect(previewDetails).toBeVisible();
    });
  }

  test("Design preview layout controls stay in sync and can switch rendering strategy", async ({ page }) => {
    await openDesignMode(page);
    const layout = designSpace(page).getByRole("region", { name: "Layout Mode" });
    const previewLayout = page.getByRole("combobox", { name: "Layout", exact: true });
    await expect(previewLayout).toHaveValue("force");
    for (const style of ["grid", "circle", "hierarchy"]) {
      await previewLayout.selectOption(style);
      await expect(layout.getByLabel("Layout style")).toHaveValue(style);
      await renderedGraph(page);
      await expect(previewLayout).toHaveValue(style);
    }
    await layout.getByLabel("Layout style").selectOption("groups");
    await renderedGraph(page);
    await expect(previewLayout).toHaveValue("groups");
    const containers = designSpace(page).getByRole("region", { name: "Containers" });
    await containers.getByLabel("Containment source").selectOption("HAS_WORK_SLICE");
    await containers.getByLabel("Render as").selectOption("region");
    await layout.getByRole("button", { name: "Container/map" }).click();
    await expect(page.getByText("5 regions / 0 collapsed", { exact: true })).toBeVisible();
    await layout.getByRole("button", { name: "Edge-network" }).click();
    await renderedGraph(page);
    await expect(page.getByText("5 regions / 0 collapsed", { exact: true })).toHaveCount(0);
    await expect(previewLayout).toBeEnabled();
  });

  test("manual arrangement marks the layout and survives visual edits and resize", async ({ page }) => {
    await openDesignMode(page);
    await page.getByRole("combobox", { name: "Layout", exact: true }).selectOption("grid");
    const graph = await renderedGraph(page);
    const node = graph.nodes.find((item) => item.id === "13")!;
    const canvas = await page.locator(".graph-canvas").boundingBox();
    await page.mouse.move(canvas!.x + node.position.x, canvas!.y + node.position.y);
    await page.mouse.down();
    await page.mouse.move(canvas!.x + node.position.x + 50, canvas!.y + node.position.y + 30, { steps: 12 });
    await page.mouse.up();
    await expect(page.getByRole("combobox", { name: "Layout", exact: true })).toHaveValue("grid");
    await expect(page.getByLabel("Arrangement moved", { exact: true })).toHaveText("*");
    const position = () => page.locator(".graph-canvas").evaluate((element) => {
      const cy = (element as HTMLElement & { _cyreg: { cy: Core } })._cyreg.cy;
      return cy.getElementById("13").position();
    });
    const arranged = await position();
    const nodeSetup = designSpace(page).getByRole("region", { name: "Node Setup" });
    await nodeSetup.getByLabel("Node model").selectOption("RoadmapItem");
    await nodeSetup.getByLabel("Colour").selectOption("flight-deck-amber");
    await renderedGraph(page);
    expect(await position()).toEqual(arranged);
    await page.setViewportSize({ width: 1280, height: 900 });
    await renderedGraph(page);
    expect(await position()).toEqual(arranged);
    await page.screenshot({ path: "test-results/design-manual-arrangement.png", fullPage: true });
    await page.getByRole("button", { name: "Restore automatic layout" }).click();
    await renderedGraph(page);
    await expect(page.getByRole("combobox", { name: "Layout", exact: true })).toHaveValue("grid");
    await expect(page.getByLabel("Arrangement moved", { exact: true })).toHaveCount(0);
  });

  for (const mode of ["Data", "Schema", "Design"]) {
    test(`dragging in ${mode} keeps the preset dropdown and offers reset`, async ({ page }) => {
      await page.getByRole("tab", { name: mode, exact: true }).click();
      const layout = page.getByRole("combobox", { name: "Layout", exact: true });
      await layout.selectOption("grid");
      const dragNode = async () => {
        const graph = await renderedGraph(page);
        const node = graph.nodes.find((item) => item.id === (mode === "Schema" ? "schema-node:RoadmapItem" : "13"))!;
        const box = await page.locator(".graph-canvas").boundingBox();
        await page.mouse.move(box!.x + node.position.x, box!.y + node.position.y);
        await page.mouse.down();
        await page.mouse.move(box!.x + node.position.x + 40, box!.y + node.position.y + 20, { steps: 10 });
        await page.mouse.up();
      };
      await dragNode();
      const marker = page.getByLabel("Arrangement moved", { exact: true });
      const reset = page.getByRole("button", { name: "Restore automatic layout" });
      await expect(layout).toHaveValue("grid");
      await expect(marker).toHaveText("*");
      await expect(reset).toBeVisible();
      await expect(page.getByText("Custom", { exact: true })).toHaveCount(0);
      await page.screenshot({ path: `test-results/moved-layout-${mode}.png`, fullPage: true });
      await reset.click();
      await renderedGraph(page);
      await expect(marker).toHaveCount(0);
      await expect(reset).toHaveCount(0);
      await expect(layout).toHaveValue("grid");
      await dragNode();
      await expect(marker).toBeVisible();
      await layout.selectOption("circle");
      await renderedGraph(page);
      await expect(marker).toHaveCount(0);
      await expect(layout).toHaveValue("circle");
    });
  }

  for (const viewport of [{ width: 1440, height: 1000 }, { width: 390, height: 844 }]) {
    test(`fits Data and Schema to the available canvas at ${viewport.width}px`, async ({ page }) => {
      await page.setViewportSize(viewport);
      for (const mode of ["Data", "Schema"]) {
        await page.getByRole("tab", { name: mode, exact: true }).click();
        await renderedGraph(page);
        const framing = await page.locator(".graph-canvas").evaluate((element) => {
          const cy = (element as HTMLElement & { _cyreg: { cy: Core } })._cyreg.cy;
          const bounds = cy.elements().renderedBoundingBox();
          return { bounds, width: cy.width(), height: cy.height(), layout: document.querySelector<HTMLSelectElement>(".layout-control select")?.value };
        });
        expect(framing.bounds.x1).toBeGreaterThanOrEqual(0);
        expect(framing.bounds.y1).toBeGreaterThanOrEqual(0);
        expect(framing.bounds.x2).toBeLessThanOrEqual(framing.width + 1);
        expect(framing.bounds.y2).toBeLessThanOrEqual(framing.height + 1);
        if (mode === "Data") {
          expect(framing.bounds.w / (framing.width - 144)).toBeGreaterThan(0.7);
          expect(framing.bounds.h / (framing.height - 144)).toBeGreaterThan(0.7);
        } else {
          expect(framing.layout).toBe("grid");
        }
        await page.screenshot({ path: `test-results/graph-fit-${mode}-${viewport.width}.png`, fullPage: true });
      }
    });
  }

  for (const viewport of [{ width: 1440, height: 1000 }, { width: 390, height: 844 }]) {
    test(`supports clear profile actions and disconnection at ${viewport.width}px`, async ({ page }) => {
      await page.setViewportSize(viewport);
      await page.getByRole("button", { name: "Change connection" }).click();
      const dialog = page.getByRole("dialog", { name: "Connection settings" });
      const fixture = dialog.getByRole("checkbox", { name: "Fixture", exact: true });
      const box = await fixture.boundingBox();
      const label = await dialog.getByText("Fixture", { exact: true }).boundingBox();
      expect(box!.width).toBeGreaterThanOrEqual(18);
      expect(label!.x).toBeGreaterThan(box!.x + box!.width);
      await page.screenshot({ path: `test-results/connection-dialog-${viewport.width}.png`, fullPage: true });
      await dialog.getByLabel("Profile name", { exact: true }).fill("Saved workspace");
      await dialog.getByRole("button", { name: "Save profile", exact: true }).click();
      await expect(dialog.getByRole("button", { name: "Saving", exact: true }).locator("svg")).toHaveClass(/icon-spin/);
      await expect(page.getByRole("status").filter({ hasText: "Profile saved" })).toBeVisible();
      await expect(dialog).toBeVisible();

      await dialog.getByRole("button", { name: "New profile", exact: true }).click();
      await expect(dialog.getByRole("button", { name: "Creating", exact: true }).locator("svg")).toHaveClass(/icon-spin/);
      await expect(page.getByRole("status").filter({ hasText: "New profile created" })).toBeVisible();
      await expect(dialog.getByLabel("Profile name", { exact: true })).toHaveValue("");
      await expect(dialog.getByLabel("Gateway URL", { exact: true })).toHaveValue("");
      await expect(dialog.getByLabel("Workspace", { exact: true })).toHaveValue("");
      await expect(fixture).not.toBeChecked();
      await expect(dialog.getByRole("button", { name: "Connect", exact: true })).toBeDisabled();
      await dialog.getByLabel("Profile name", { exact: true }).fill("Second workspace");
      await dialog.getByLabel("Workspace", { exact: true }).fill("flight-deck-demo");
      await fixture.check();
      await dialog.getByRole("button", { name: "Save profile", exact: true }).click();
      await expect(page.getByRole("status").filter({ hasText: "Profile saved" })).toBeVisible();
      await dialog.getByRole("button", { name: "Connect", exact: true }).click();
      await expect(dialog.getByRole("button", { name: "Connecting", exact: true }).locator("svg")).toHaveClass(/icon-spin/);
      await expect(dialog).toHaveCount(0);
      await expect(page.getByRole("status").filter({ hasText: "Connected" })).toBeVisible();
      await expect(page.getByText(/fixture snapshot:/i)).toBeVisible();

      await page.getByRole("button", { name: "Change connection" }).click();
      await dialog.getByRole("button", { name: "Remove profile", exact: true }).click();
      await expect(dialog).toHaveCount(0);
      await expect(page.getByText("No connection", { exact: true })).toBeVisible();
      await expect(page.getByText("Not connected.", { exact: true })).toBeVisible();
      await expect(page.getByRole("status").filter({ hasText: "Profile removed" })).toBeVisible();
      const persisted = await page.evaluate((key) => JSON.parse(localStorage.getItem(key) ?? "{}"), STORE_STORAGE_KEY);
      expect(persisted.selectedProfileId).toBe("");
      expect(persisted.profiles.map((profile: { name: string }) => profile.name)).toEqual(["Saved workspace"]);

      await page.reload();
      await expect(page.getByText("No connection", { exact: true })).toBeVisible();
      await expect(page.getByText(/fixture snapshot:/i)).toHaveCount(0);
      await page.getByRole("button", { name: "Change connection" }).click();
      await dialog.getByLabel("Profile", { exact: true }).selectOption("local-workspace");
      await dialog.getByRole("button", { name: "Remove profile", exact: true }).click();
      await expect(dialog).toHaveCount(0);
      await page.reload();
      await expect(page.getByText("No connection", { exact: true })).toBeVisible();
      await page.getByRole("button", { name: "Change connection" }).click();
      await expect(dialog.getByLabel("Profile", { exact: true })).toHaveValue("");
      await expect(dialog.getByLabel("Profile", { exact: true }).locator("option")).toHaveText(["No profiles"]);
      await expect(dialog.getByRole("button", { name: "New profile", exact: true })).toBeEnabled();
    });
  }

  test("a failed datasource connection keeps the dialog open and permits retry", async ({ page }) => {
    await page.route("**/api/**", (route) => route.fulfill({ status: 503, contentType: "text/plain", body: "Datasource unavailable" }));
    await page.getByRole("button", { name: "Change connection" }).click();
    const dialog = page.getByRole("dialog", { name: "Connection settings" });
    await dialog.getByLabel("Fixture").uncheck();
    await dialog.getByRole("button", { name: "Connect", exact: true }).click();
    await expect(dialog.getByRole("alert")).toContainText("snapshot request failed: HTTP 503");
    await expect(dialog).toBeVisible();
    await expect(dialog.getByRole("button", { name: "Connect", exact: true })).toBeEnabled();
    const errors = pageErrors.get(page) ?? [];
    expect(errors.every((message) => message.includes("503"))).toBeTruthy();
    errors.length = 0;
    await dialog.getByLabel("Fixture").check();
    await dialog.getByRole("button", { name: "Connect", exact: true }).click();
    await expect(dialog).toHaveCount(0);
    await expect(page.getByText(/fixture snapshot:/i)).toBeVisible();
  });

  test("uses keyboard-accessible tabs with a dedicated Design workspace", async ({ page }) => {
    const tabs = page.getByRole("tablist", { name: "Workspace mode" });
    await expect(tabs.getByRole("tab")).toHaveCount(4);
    await tabs.getByRole("tab", { name: "Data", exact: true }).focus();
    await page.keyboard.press("ArrowRight");
    await expect(tabs.getByRole("tab", { name: "Schema", exact: true })).toBeFocused();
    await expect(page.getByRole("tabpanel", { name: "Schema", exact: true })).toBeVisible();
    await expect(designSpace(page)).toHaveCount(0);
    await page.keyboard.press("End");
    await expect(tabs.getByRole("tab", { name: "Design", exact: true })).toBeFocused();
    await expect(tabs.getByRole("tab", { name: "Design", exact: true })).toHaveAttribute("aria-selected", "true");
    await expect(designSpace(page)).toBeVisible();
    await expect(page.getByRole("heading", { name: "Preview", exact: true })).toBeVisible();
    await expect.poll(async () => (await renderedGraph(page)).paintedPixels).toBeGreaterThan(100);
    await page.screenshot({ path: "test-results/design-workspace-desktop.png", fullPage: true });
    await page.keyboard.press("ArrowRight");
    await expect(tabs.getByRole("tab", { name: "Data", exact: true })).toBeFocused();
    await expect(designSpace(page)).toHaveCount(0);
    await page.keyboard.press("ArrowLeft");
    await expect(tabs.getByRole("tab", { name: "Design", exact: true })).toBeFocused();
    await page.setViewportSize({ width: 390, height: 844 });
    await expect(designSpace(page)).toBeVisible();
    await expect(page.getByRole("tabpanel", { name: "Design", exact: true })).toBeVisible();
    const sizing = await page.evaluate(() => ({ width: document.documentElement.clientWidth, content: document.documentElement.scrollWidth }));
    expect(sizing.content).toBeLessThanOrEqual(sizing.width);
    await renderedGraph(page);
    await page.screenshot({ path: "test-results/design-workspace-mobile.png", fullPage: true });
  });

  test("applies Node Setup overrides to local visual projection", async ({ page }) => {
    await openDesignMode(page);
    const design = designSpace(page);
    const nodeSetup = design.getByRole("region", { name: "Node Setup" });

    await nodeSetup.getByLabel("Node model").selectOption("RoadmapItem");
    await nodeSetup.getByLabel("Display name").fill("PM roadmap");
    await nodeSetup.getByLabel("Colour").selectOption("flight-deck-amber");
    await nodeSetup.getByLabel("Glyph / shape").selectOption("card");
    await nodeSetup.getByLabel("Detail density").selectOption("rich");
    await nodeSetup.getByLabel("Visual role").selectOption("anchor");

    await expect.poll(async () => (await renderedGraph(page)).nodes.find((node) => node.id === "13")?.label).toContain("PM roadmap");
    await expect(nodeSetup.getByLabel("Glyph / shape")).toHaveValue("card");
    await expect(nodeSetup.getByLabel("Detail density")).toHaveValue("rich");
    await expect(nodeSetup.getByLabel("Visual role")).toHaveValue("anchor");
    await expect.poll(async () => (await renderedGraph(page)).nodes.find((node) => node.id === "13")?.color).toBe("#d19a4a");
    await expect(graphCanvas(page)).toBeVisible();
  });

  test("applies Edge Links overrides including label visibility", async ({ page }) => {
    await openDesignMode(page);
    const edgeLinks = designSpace(page).getByRole("region", { name: "Edge Links" });

    await edgeLinks.getByLabel("Edge model").selectOption("HAS_WORK_SLICE");
    await edgeLinks.getByLabel("Link label").fill("contains work");
    await edgeLinks.getByLabel("Link style").selectOption("dependency");
    await edgeLinks.getByLabel("Direction emphasis").selectOption("strong");
    await edgeLinks.getByLabel("Line weight").selectOption("strong");
    await edgeLinks.getByLabel("Line style").selectOption("dashed");
    await edgeLinks.getByLabel("Label visibility").selectOption("always");

    await expect.poll(async () => (await renderedGraph(page)).edges.find((edge) => edge.id === "e2")?.label).toBe("contains work");
    await expect(edgeLinks.getByLabel("Link style")).toHaveValue("dependency");
    await expect(edgeLinks.getByLabel("Direction emphasis")).toHaveValue("strong");
    await expect(edgeLinks.getByLabel("Line weight")).toHaveValue("strong");
    await expect(edgeLinks.getByLabel("Line style")).toHaveValue("dashed");
    await expect(edgeLinks.getByLabel("Label visibility")).toHaveValue("always");
    await expect(graphCanvas(page)).toBeVisible();
  });

  test("supports advisory containers and layout mode switching", async ({ page }) => {
    await openDesignMode(page);
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

  for (const viewport of [{ width: 1440, height: 1000 }, { width: 390, height: 844 }]) {
    test(`renders and collapses relationship regions at ${viewport.width}px`, async ({ page }) => {
      await page.setViewportSize(viewport);
      await openDesignMode(page);
      const design = designSpace(page);
      const containers = design.getByRole("region", { name: "Containers" });
      await containers.getByLabel("Containment source").selectOption("HAS_WORK_SLICE");
      await containers.getByLabel("Render as").selectOption("lane");
      await containers.getByLabel("Collapse affordance").selectOption("expanded");
      await design.getByRole("button", { name: "Container/map" }).click();
      await page.getByLabel("Workspace mode").getByRole("tab", { name: "Data" }).click();
      await expect(page.getByText("5 regions / 0 collapsed", { exact: true })).toBeVisible();
      await expect(page.getByText("Rendering graph", { exact: true })).toHaveCount(0);
      const expanded = await renderedGraph(page);
      expect(expanded.nodes).toHaveLength(fixtureSnapshot.nodes.length + 5);
      expect(expanded.nodes.find((node) => node.id === "566")?.parent).toBe("visual-container:13");
      expect(expanded.nodes.find((node) => node.id === "visual-container:13")?.classes).toContain("render-lane");
      expect(expanded.edges).toHaveLength(fixtureSnapshot.edges.length);
      expect(expanded.paintedPixels).toBeGreaterThan(100);
      await page.locator(".graph-shell").screenshot({ path: `test-results/container-map-expanded-${viewport.width}.png` });

      await openDesignMode(page);
      await containers.getByLabel("Containment source").selectOption("HAS_WORK_SLICE");
      await containers.getByLabel("Collapse affordance").selectOption("collapsed");
      await page.getByLabel("Workspace mode").getByRole("tab", { name: "Data" }).click();
      await expect(page.getByText("5 regions / 5 collapsed", { exact: true })).toBeVisible();
      await expect(page.getByText("Rendering graph", { exact: true })).toHaveCount(0);
      const collapsed = await renderedGraph(page);
      expect(collapsed.nodes.map((node) => node.id)).not.toContain("566");
      expect(collapsed.nodes.map((node) => node.id)).not.toContain("13");
      expect(collapsed.edges.find((edge) => edge.id === "e1")?.target).toBe("visual-container:13");
      expect(collapsed.edges.find((edge) => edge.id === "e3")?.source).toBe("visual-container:13");
      expect(collapsed.edges.map((edge) => edge.id)).not.toContain("e2");
      expect(collapsed.paintedPixels).toBeGreaterThan(100);
      await page.locator(".graph-shell").screenshot({ path: `test-results/container-map-collapsed-${viewport.width}.png` });
      const region = collapsed.nodes.find((node) => node.id === "visual-container:13")!;
      await page.locator(".graph-canvas").click({ position: region.position });
      const selection = page.getByRole("complementary", { name: "Selection" });
      await expect(selection.getByRole("heading", { name: "Build the GRM flight-deck", exact: true })).toBeVisible();
      await expect(selection.getByText("node / roadmap item", { exact: true })).toBeVisible();
      await expect(selection.getByText("active", { exact: true })).toBeVisible();

      await page.reload();
      await expect(page.getByText("5 regions / 5 collapsed", { exact: true })).toBeVisible({ timeout: 15_000 });
      await openDesignMode(page);
      await containers.getByLabel("Containment source").selectOption("HAS_WORK_SLICE");
      await containers.getByLabel("Collapse affordance").selectOption("expanded");
      await design.getByRole("button", { name: "Edge-network" }).click();
      await page.getByLabel("Workspace mode").getByRole("tab", { name: "Data" }).click();
      await expect(page.locator(".container-map-summary")).toHaveCount(0);
      await expect(page.getByText("Rendering graph", { exact: true })).toHaveCount(0);
      const restored = await renderedGraph(page);
      expect(restored.nodes).toHaveLength(fixtureSnapshot.nodes.length);
      expect(restored.edges).toHaveLength(fixtureSnapshot.edges.length);
      expect(restored.nodes.every((node) => !node.parent)).toBeTruthy();
    });
  }

  test("persists local overlays by profile and workspace only", async ({ page }) => {
    await openDesignMode(page);
    await makeNodeAndContainerOverride(page, "PM roadmap");

    await page.reload();
    await expect(page.getByText(/fixture snapshot:/i)).toBeVisible({ timeout: 15_000 });
    await openDesignMode(page);
    await designSpace(page).getByRole("region", { name: "Node Setup" }).getByLabel("Node model").selectOption("RoadmapItem");
    await expect(designSpace(page).getByRole("region", { name: "Node Setup" }).getByLabel("Display name")).toHaveValue("PM roadmap");
    await expect.poll(async () => (await renderedGraph(page)).nodes.some((node) => node.label.includes("PM roadmap"))).toBeTruthy();
    await expect(designSpace(page).getByRole("region", { name: "Layout Mode" }).getByRole("button", { name: "Container/map" })).toHaveClass(/active/);

    await changeWorkspace(page, "another-memory");
    await openDesignMode(page);
    await expect.poll(async () => (await renderedGraph(page)).nodes.some((node) => node.label.includes("PM roadmap"))).toBeFalsy();
    await expect(designSpace(page).getByRole("region", { name: "Node Setup" }).getByLabel("Display name")).toHaveValue("");
    await expect(designSpace(page).getByRole("region", { name: "Layout Mode" }).getByRole("button", { name: "Edge-network" })).toHaveClass(/active/);

    await createProfile(page, "Second profile", "flight-deck-demo");
    await openDesignMode(page);
    await expect.poll(async () => (await renderedGraph(page)).nodes.some((node) => node.label.includes("PM roadmap"))).toBeFalsy();
  });

  test("resets node, edge, container, and layout overrides to generated defaults", async ({ page }) => {
    await openDesignMode(page);
    await makeNodeEdgeContainerAndLayoutOverrides(page);

    await designSpace(page).getByRole("button", { name: "Reset to generated defaults" }).click();

    const design = designSpace(page);
    await expect.poll(async () => (await renderedGraph(page)).nodes.some((node) => node.label.includes("PM roadmap"))).toBeFalsy();
    await expect.poll(async () => (await renderedGraph(page)).edges.some((edge) => edge.label === "contains work")).toBeFalsy();
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

    await openDesignMode(page);
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
    await openDesignMode(page);
    await makeNodeEdgeContainerAndLayoutOverrides(page);

    const workspaceMode = page.getByLabel("Workspace mode");
    await workspaceMode.getByRole("tab", { name: "Data" }).click();
    await expect(page.getByLabel("Data query and filter")).toBeVisible();
    await expect(graphCanvas(page)).toBeVisible();

    await workspaceMode.getByRole("tab", { name: "Schema" }).click();
    await expect(designSpace(page)).toHaveCount(0);
    await expect(schemaPanel(page)).toBeVisible();
    await expect(graphCanvas(page)).toBeVisible();

    await workspaceMode.getByRole("tab", { name: "Audit" }).click();
    await expect(page.getByRole("heading", { name: "Audit" })).toBeVisible();
    await expect(page.getByPlaceholder("principal, workspace, outcome, operation")).toBeVisible();
    await expect(page.getByText("bounded snapshot read")).toBeVisible();
  });

  test("every node glyph changes the rendered shape and dimensions", async ({ page }) => {
    await openDesignMode(page);
    const controls = designSpace(page).getByRole("region", { name: "Node Setup" });
    await controls.getByLabel("Node model").selectOption("RoadmapItem");
    for (const [option, shape, width, height] of [
      ["card", "round-rectangle", 36, 22], ["hex", "hexagon", 16, 16],
      ["diamond", "diamond", 16, 16], ["lane", "round-rectangle", 58, 20],
      ["dot", "ellipse", 16, 16], ["generated", "ellipse", 16, 16]
    ] as const) {
      await test.step(option, async () => {
        await controls.getByLabel("Glyph / shape").selectOption(option);
        const graph = await renderedGraph(page);
        const node = graph.nodes.find((item) => item.id === "13")!;
        expect(node).toMatchObject({ shape, width, height });
        expect(graph.nodes.find((item) => item.id === "566")?.shape).toBe("ellipse");
        expect(graph.paintedPixels).toBeGreaterThan(100);
      });
    }
  });

  test("all detail density options resize cards without changing node properties", async ({ page }) => {
    await openDesignMode(page);
    const controls = designSpace(page).getByRole("region", { name: "Node Setup" });
    await controls.getByLabel("Node model").selectOption("RoadmapItem");
    await controls.getByLabel("Glyph / shape").selectOption("card");
    for (const option of ["rich", "compact", "standard", "generated"]) {
      await test.step(option, async () => {
        await controls.getByLabel("Detail density").selectOption(option);
        const node = (await renderedGraph(page)).nodes.find((item) => item.id === "13")!;
        expect(node.width).toBe(option === "rich" ? 46 : 36);
        expect(node.height).toBe(option === "rich" ? 28 : 22);
        expect(node.props).toEqual(fixtureSnapshot.nodes.find((item) => item.id === "13")!.props);
      });
    }
  });

  test("all visual roles apply the expected border treatment", async ({ page }) => {
    await openDesignMode(page);
    const controls = designSpace(page).getByRole("region", { name: "Node Setup" });
    await controls.getByLabel("Node model").selectOption("RoadmapItem");
    for (const [option, color] of [
      ["anchor", "rgb(255,209,102)"], ["risk", "rgb(255,106,61)"],
      ["evidence", "rgb(137,213,155)"], ["actor", "rgb(198,168,255)"],
      ["decision", "rgb(213,248,255)"], ["context", "rgb(213,248,255)"],
      ["object", "rgb(213,248,255)"], ["process", "rgb(213,248,255)"],
      ["generated", "rgb(213,248,255)"]
    ]) {
      await test.step(option, async () => {
        await controls.getByLabel("Visual role").selectOption(option);
        const node = (await renderedGraph(page)).nodes.find((item) => item.id === "13")!;
        expect(node.borderColor).toBe(color);
        expect(node.borderWidth).toBe(option === "generated" ? 1 : 2.2);
        expect(node.borderOpacity).toBe(option === "generated" ? 0.32 : 0.88);
      });
    }
  });

  test("all palette options and grouping affect only the chosen node model", async ({ page }) => {
    await openDesignMode(page);
    const controls = designSpace(page).getByRole("region", { name: "Node Setup" });
    await controls.getByLabel("Node model").selectOption("RoadmapItem");
    const unchanged = (await renderedGraph(page)).nodes.find((item) => item.id === "566")!;
    await controls.getByLabel("Group / lane").fill("Delivery lane");
    for (const [token, color] of [
      ["flight-deck-blue", "#2f9fd0"], ["flight-deck-teal", "#3aa7a3"],
      ["flight-deck-indigo", "#5e83d8"], ["flight-deck-green", "#2dbe8f"],
      ["flight-deck-violet", "#8f7be8"], ["flight-deck-amber", "#d19a4a"],
      ["flight-deck-rose", "#d46d7d"], ["flight-deck-sky", "#6db6e8"], ["", "#2f9fd0"]
    ]) {
      await test.step(token || "Generated", async () => {
        await controls.getByLabel("Colour").selectOption(token);
        const graph = await renderedGraph(page);
        expect(graph.nodes.find((item) => item.id === "13")).toMatchObject({ color, group: "Delivery lane" });
        expect(graph.nodes.find((item) => item.id === "566")).toMatchObject({ color: unchanged.color, group: unchanged.group });
      });
    }
  });

  test("every edge style applies its rendered stroke treatment", async ({ page }) => {
    await openDesignMode(page);
    const controls = designSpace(page).getByRole("region", { name: "Edge Links" });
    await controls.getByLabel("Edge model").selectOption("HAS_WORK_SLICE");
    for (const [option, color, width, lineStyle] of [
      ["dependency", "#ffd166", 1.7, "dashed"], ["evidence", "#89d59b", 1.7, "dotted"],
      ["warning", "#ff6a3d", 2.4, "solid"], ["directed", "#536675", 1, "solid"],
      ["generated", "#536675", 1, "solid"]
    ] as const) {
      await test.step(option, async () => {
        await controls.getByLabel("Link style").selectOption(option);
        const graph = await renderedGraph(page);
        expect(graph.edges.find((item) => item.id === "e2")).toMatchObject({ color, width, lineStyle });
        expect(graph.edges.find((item) => item.id === "e1")?.color).toBe("#536675");
      });
    }
  });

  test("every direction emphasis changes rendered opacity and arrow size", async ({ page }) => {
    await openDesignMode(page);
    const controls = designSpace(page).getByRole("region", { name: "Edge Links" });
    await controls.getByLabel("Edge model").selectOption("HAS_WORK_SLICE");
    for (const [option, opacity, arrowScale] of [
      ["strong", 0.9, 0.9], ["muted", 0.32, 0.45], ["normal", 0.56, 0.62], ["generated", 0.56, 0.62]
    ] as const) {
      await test.step(option, async () => {
        await controls.getByLabel("Direction emphasis").selectOption(option);
        expect((await renderedGraph(page)).edges.find((item) => item.id === "e2")).toMatchObject({ opacity, arrowScale });
      });
    }
  });

  test("every line weight controls rendered edge width", async ({ page }) => {
    await openDesignMode(page);
    const controls = designSpace(page).getByRole("region", { name: "Edge Links" });
    await controls.getByLabel("Edge model").selectOption("HAS_WORK_SLICE");
    for (const [option, width] of [["fine", 0.8], ["strong", 2.4], ["normal", 1], ["generated", 1]] as const) {
      await test.step(option, async () => {
        await controls.getByLabel("Line weight").selectOption(option);
        expect((await renderedGraph(page)).edges.find((item) => item.id === "e2")?.width).toBe(width);
      });
    }
  });

  test("every line style controls the rendered stroke pattern", async ({ page }) => {
    await openDesignMode(page);
    const controls = designSpace(page).getByRole("region", { name: "Edge Links" });
    await controls.getByLabel("Edge model").selectOption("HAS_WORK_SLICE");
    for (const option of ["dashed", "dotted", "solid", "generated"]) {
      await test.step(option, async () => {
        await controls.getByLabel("Line style").selectOption(option);
        expect((await renderedGraph(page)).edges.find((item) => item.id === "e2")?.lineStyle).toBe(option === "generated" ? "solid" : option);
      });
    }
  });

  test("all edge label visibility options preserve canonical edge properties", async ({ page }) => {
    await openDesignMode(page);
    const controls = designSpace(page).getByRole("region", { name: "Edge Links" });
    await controls.getByLabel("Edge model").selectOption("HAS_WORK_SLICE");
    await controls.getByLabel("Link label").fill("delivers");
    for (const option of ["always", "hidden", "self-loops", "generated"]) {
      await test.step(option, async () => {
        await controls.getByLabel("Label visibility").selectOption(option);
        const edge = (await renderedGraph(page)).edges.find((item) => item.id === "e2")!;
        expect(edge.displayLabel).toBe(option === "always" ? "delivers" : "");
        expect(edge.label).toBe("delivers");
        expect(edge.props).toEqual({ ...fixtureSnapshot.edges.find((item) => item.id === "2")!.props, selfLoop: false });
      });
    }
  });

  test("every container presentation and collapse option preserves relationships", async ({ page }) => {
    await openDesignMode(page);
    const containers = designSpace(page).getByRole("region", { name: "Containers" });
    await containers.getByLabel("Containment source").selectOption("HAS_WORK_SLICE");
    await designSpace(page).getByRole("region", { name: "Layout Mode" }).getByRole("button", { name: "Container/map" }).click();
    for (const option of ["region", "card", "lane", "section", "generated"]) {
      await test.step(option, async () => {
        await containers.getByLabel("Render as").selectOption(option);
        const graph = await renderedGraph(page);
        expect(graph.nodes.filter((item) => item.classes.includes("visual-container"))).toHaveLength(5);
        expect(graph.nodes.find((item) => item.id === "visual-container:13")?.classes).toContain(`render-${option}`);
        expect(graph.nodes.find((item) => item.id === "566")?.parent).toBe("visual-container:13");
        expect(graph.edges).toHaveLength(fixtureSnapshot.edges.length);
        expect(graph.paintedPixels).toBeGreaterThan(100);
      });
    }
    for (const option of ["collapsed", "expanded", "generated"]) {
      await test.step(option, async () => {
        await containers.getByLabel("Collapse affordance").selectOption(option);
        const graph = await renderedGraph(page);
        expect(graph.nodes.some((item) => item.id === "566")).toBe(option !== "collapsed");
        expect(graph.edges.find((item) => item.id === "e1")?.target).toBe(option === "collapsed" ? "visual-container:13" : "13");
      });
    }
  });

  test("self-loop label visibility distinguishes loops from ordinary dependencies", async ({ page }) => {
    const slices = fixtureSnapshot.nodes.filter((node) => node.model === "WorkSlice");
    const edgeHint = {
      model: "DEPENDS_ON", label: "depends on", fromModel: "WorkSlice", toModel: "WorkSlice",
      group: "edge:WorkSlice->WorkSlice", styleToken: "directed", directionEmphasis: "directed", detailFields: ["reason"]
    };
    const snapshot = {
      ...fixtureSnapshot,
      edgeModels: [...fixtureSnapshot.edgeModels, "DEPENDS_ON"],
      schemaEdges: [...(fixtureSnapshot.schemaEdges ?? []), { model: "DEPENDS_ON", fromModel: "WorkSlice", toModel: "WorkSlice", fields: [] }],
      edges: [...fixtureSnapshot.edges,
        { id: "loop", model: "DEPENDS_ON", from: slices[0].id, to: slices[0].id, props: { reason: "Revisit this step" } },
        { id: "dependency", model: "DEPENDS_ON", from: slices[0].id, to: slices[1].id, props: { reason: "Continue to the next step" } }
      ]
    };
    await page.route("**/api/**", async (route) => {
      const path = new URL(route.request().url()).pathname;
      const payload = path.endsWith("/snapshot") ? snapshot
        : path.endsWith("/visual-projection") ? { ...fixtureVisualProjection, edgeModels: [...fixtureVisualProjection.edgeModels, edgeHint] }
        : path.endsWith("/audit/status") ? fixtureSecurityAuditStatus : fixtureSecurityStatus;
      await route.fulfill({ json: payload });
    });
    await page.getByRole("button", { name: "Change connection" }).click();
    const dialog = page.getByRole("dialog", { name: "Connection settings" });
    await dialog.getByLabel("Fixture").uncheck();
    await dialog.getByRole("button", { name: "Connect", exact: true }).click();
    await expect(dialog).toHaveCount(0);
    await openDesignMode(page);
    const controls = designSpace(page).getByRole("region", { name: "Edge Links" });
    await controls.getByLabel("Edge model").selectOption("DEPENDS_ON");
    await controls.getByLabel("Link label").fill("next step");
    for (const option of ["self-loops", "always", "hidden", "generated"]) {
      await test.step(option, async () => {
        await controls.getByLabel("Label visibility").selectOption(option);
        const graph = await renderedGraph(page);
        expect(graph.edges.find((edge) => edge.id === "eloop")?.displayLabel).toBe(option === "hidden" ? "" : "next step");
        expect(graph.edges.find((edge) => edge.id === "edependency")?.displayLabel).toBe(option === "always" ? "next step" : "");
      });
    }
  });

  for (const viewport of [{ width: 1440, height: 1000 }, { width: 390, height: 844 }]) {
    test(`all Design layouts render and survive tab switches at ${viewport.width}px`, async ({ page }) => {
      await page.setViewportSize(viewport);
      await openDesignMode(page);
      const layout = designSpace(page).getByRole("region", { name: "Layout Mode" });
      for (const option of ["grid", "circle", "hierarchy", "groups", "force", "generated"]) {
        await test.step(option, async () => {
          await layout.getByLabel("Layout style").selectOption(option);
          const graph = await renderedGraph(page);
          expect(graph.nodes).toHaveLength(fixtureSnapshot.nodes.length);
          expect(graph.edges).toHaveLength(fixtureSnapshot.edges.length);
          expect(graph.paintedPixels).toBeGreaterThan(100);
          expect(graph.nodes.every((item) => Number.isFinite(item.position.x) && Number.isFinite(item.position.y))).toBeTruthy();
          await page.getByRole("tab", { name: "Data", exact: true }).click();
          await renderedGraph(page);
          await expect(page.getByRole("combobox", { name: "Layout", exact: true })).toHaveValue(option === "generated" ? "force" : option);
          await openDesignMode(page);
          await renderedGraph(page);
          await expect(layout.getByLabel("Layout style")).toHaveValue(option);
        });
      }
      await layout.getByLabel("Layout style").selectOption("circle");
      await renderedGraph(page);
      await page.reload();
      await page.getByText(/fixture snapshot:/i).waitFor();
      await openDesignMode(page);
      await renderedGraph(page);
      await expect(layout.getByLabel("Layout style")).toHaveValue("circle");
      await expect(page.getByRole("combobox", { name: "Layout", exact: true })).toHaveValue("circle");
    });
  }

  test("Fit restores graph framing without changing node positions", async ({ page }) => {
    await renderedGraph(page);
    const positions = await page.locator(".graph-canvas").evaluate((element) => {
      const cy = (element as HTMLElement & { _cyreg: { cy: Core } })._cyreg.cy;
      const positions = cy.nodes().map((node) => ({ id: node.id(), ...node.position() }));
      cy.zoom(4);
      cy.pan({ x: -1000, y: -1000 });
      return positions;
    });
    await page.getByRole("button", { name: "Fit", exact: true }).click();
    const framed = await page.locator(".graph-canvas").evaluate((element) => {
      const cy = (element as HTMLElement & { _cyreg: { cy: Core } })._cyreg.cy;
      return { positions: cy.nodes().map((node) => ({ id: node.id(), ...node.position() })), bounds: cy.elements().renderedBoundingBox(), width: cy.width(), height: cy.height() };
    });
    expect(framed.positions).toEqual(positions);
    expect(framed.bounds.x1).toBeGreaterThanOrEqual(0);
    expect(framed.bounds.y1).toBeGreaterThanOrEqual(0);
    expect(framed.bounds.x2).toBeLessThanOrEqual(framed.width);
    expect(framed.bounds.y2).toBeLessThanOrEqual(framed.height);
  });

  test("selection glow follows every node glyph without clipping its blur", async ({ page }) => {
    await openDesignMode(page);
    const controls = designSpace(page).getByRole("region", { name: "Node Setup" });
    await controls.getByLabel("Node model").selectOption("RoadmapItem");
    for (const [glyph, shape] of [
      ["dot", "ellipse"], ["card", "round-rectangle"], ["hex", "hexagon"],
      ["diamond", "diamond"], ["lane", "round-rectangle"], ["generated", "ellipse"]
    ]) {
      await test.step(glyph, async () => {
        await controls.getByLabel("Glyph / shape").selectOption(glyph);
        await renderedGraph(page);
        await selectForGlow(page, "13");
        const appearance = await selectionGlow(page);
        expect(appearance.shape).toBe(shape);
        expect(appearance.blur).toContain("blur(8px)");
        expect(appearance.outerClip).toBe("none");
        if (shape === "ellipse") expect(appearance.radius).toBe("50%");
        if (shape === "round-rectangle") expect(Number.parseFloat(appearance.radius)).toBeGreaterThan(0);
        if (shape === "diamond" || shape === "hexagon") expect(appearance.innerClip).toContain("polygon");
        await page.screenshot({ path: `test-results/selection-glow-${glyph}.png`, fullPage: true });
      });
    }
  });

  for (const viewport of [{ width: 1440, height: 1000 }, { width: 390, height: 844 }]) {
    test(`wide container selection glow follows rounded corners at ${viewport.width}px`, async ({ page }) => {
      await page.setViewportSize(viewport);
      await openDesignMode(page);
      const containers = designSpace(page).getByRole("region", { name: "Containers" });
      await containers.getByLabel("Containment source").selectOption("HAS_WORK_SLICE");
      await containers.getByLabel("Render as").selectOption("card");
      await containers.getByLabel("Collapse affordance").selectOption("expanded");
      await designSpace(page).getByRole("region", { name: "Layout Mode" }).getByRole("button", { name: "Container/map" }).click();
      await page.getByRole("tab", { name: "Data", exact: true }).click();
      await renderedGraph(page);
      await selectForGlow(page, "visual-container:13");
      const appearance = await selectionGlow(page);
      expect(appearance.shape).toBe("round-rectangle");
      expect(appearance.width).toBeGreaterThan(appearance.height);
      expect(Number.parseFloat(appearance.radius)).toBeGreaterThan(0);
      expect(appearance.outerClip).toBe("none");
      await page.locator(".graph-shell").screenshot({ path: `test-results/selection-glow-container-${viewport.width}.png` });
    });
  }

  test("Schema selection glow follows model-card corners and clears on deselection", async ({ page }) => {
    await openSchemaMode(page);
    await renderedGraph(page);
    await selectForGlow(page, "schema-node:RoadmapItem");
    const appearance = await selectionGlow(page);
    expect(appearance.shape).toBe("round-rectangle");
    expect(Number.parseFloat(appearance.radius)).toBeGreaterThan(0);
    await page.locator(".graph-canvas").evaluate((element) => {
      (element as HTMLElement & { _cyreg: { cy: Core } })._cyreg.cy.nodes().unselect();
    });
    await expect(page.locator(".selection-reticule")).toHaveCSS("opacity", "0");
  });
});

async function selectForGlow(page: Page, id: string) {
  await page.locator(".graph-canvas").evaluate((element, nodeId) => {
    const cy = (element as HTMLElement & { _cyreg: { cy: Core } })._cyreg.cy;
    cy.nodes().unselect();
    const node = cy.getElementById(nodeId);
    node.select();
    cy.zoom(node.isParent() ? 0.65 : 2);
    cy.center(node);
  }, id);
  await expect(page.locator(".selection-reticule")).toHaveCSS("opacity", "1");
}

async function selectionGlow(page: Page) {
  return page.locator(".selection-reticule").evaluate((element) => {
    const glow = element.querySelector(".selection-glow")!;
    const outer = getComputedStyle(glow);
    const inner = getComputedStyle(glow, "::before");
    const box = glow.getBoundingClientRect();
    return { shape: (element as HTMLElement).dataset.shape, radius: inner.borderRadius,
      innerClip: inner.clipPath, outerClip: outer.clipPath, blur: outer.filter, width: box.width, height: box.height };
  });
}

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
  await workspaceMode.getByRole("tab", { name: "Schema" }).click();
  await expect(workspaceMode.getByRole("tab", { name: "Schema" })).toHaveClass(/active/);
  await expect(schemaPanel(page)).toBeVisible();
  await expect(designSpace(page)).toHaveCount(0);
  await expect(graphCanvas(page)).toBeVisible();
}

async function openDesignMode(page: Page) {
  await page.getByRole("tab", { name: "Design", exact: true }).click();
  await expect(page.getByRole("tabpanel", { name: "Design", exact: true })).toBeVisible();
  await expect(designSpace(page)).toBeVisible();
  await expect(schemaPanel(page)).toHaveCount(0);
  await expect(graphCanvas(page)).toBeVisible();
}

function graphCanvas(page: Page): Locator {
  return page.locator(".graph-canvas canvas").first();
}

async function renderedGraph(page: Page) {
  await page.waitForFunction(() => {
    const element = document.querySelector(".graph-canvas") as (HTMLElement & { _cyreg?: { cy?: Core } }) | null;
    return element?._cyreg?.cy && !element._cyreg.cy.destroyed() && !document.querySelector(".rendering-overlay");
  });
  return page.locator(".graph-canvas").evaluate((element) => {
    const cy = (element as HTMLElement & { _cyreg: { cy: Core } })._cyreg.cy;
    let paintedPixels = 0;
    for (const canvas of element.querySelectorAll("canvas")) {
      const context = canvas.getContext("2d");
      if (!context) continue;
      const pixels = context.getImageData(0, 0, canvas.width, canvas.height).data;
      for (let offset = 3; offset < pixels.length; offset += 4) {
        if (pixels[offset] > 0) paintedPixels += 1;
      }
    }
    return {
      nodes: cy.nodes().map((node) => ({
        id: node.id(), label: String(node.data("label")), color: String(node.data("color")),
        parent: node.data("parent") as string | undefined, classes: node.classes(), position: node.renderedPosition(),
        shape: node.style("shape"), width: node.width(), height: node.height(), group: node.data("group"),
        borderColor: node.style("border-color"), borderWidth: Number.parseFloat(node.style("border-width")),
        borderOpacity: Number.parseFloat(node.style("border-opacity")), props: node.data("props")
      })),
      edges: cy.edges().map((edge) => ({
        id: edge.id(), label: String(edge.data("label")), source: edge.source().id(), target: edge.target().id(),
        displayLabel: edge.style("label"), color: edge.data("edgeColor"), width: Number.parseFloat(edge.style("width")),
        lineStyle: edge.style("line-style"), opacity: Number.parseFloat(edge.style("opacity")),
        arrowScale: Number.parseFloat(edge.style("arrow-scale")), props: edge.data("props")
      })),
      paintedPixels
    };
  });
}

function schemaPanel(page: Page): Locator {
  return page.getByRole("complementary", { name: "Schema and projection" });
}

function designSpace(page: Page): Locator {
  return page.getByRole("region", { name: "Visual Design Space" });
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
  await expect(dialog).toHaveCount(0);
  await expect(page.getByText(/fixture snapshot:/i)).toBeVisible({ timeout: 15_000 });
}

async function createProfile(page: Page, profileName: string, workspace: string) {
  await page.getByRole("button", { name: "Change connection" }).click();
  const dialog = page.getByRole("dialog", { name: "Connection settings" });
  await dialog.getByRole("button", { name: "New profile" }).click();
  await expect(page.getByRole("status").filter({ hasText: "New profile created" })).toBeVisible();
  await dialog.getByRole("textbox", { name: "Profile name" }).fill(profileName);
  await dialog.getByRole("textbox", { name: "Workspace" }).fill(workspace);
  await dialog.getByLabel("Fixture").check();
  await dialog.getByRole("button", { name: "Save profile" }).click();
  await expect(page.getByRole("status").filter({ hasText: "Profile saved" })).toBeVisible();
  await dialog.getByRole("button", { name: "Connect", exact: true }).click();
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
