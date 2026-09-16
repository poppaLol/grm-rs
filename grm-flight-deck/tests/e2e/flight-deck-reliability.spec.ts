import { expect, test } from "@playwright/test";

const storeKey = "grm-flight-deck.graph-store.v1";
const secretLike = [
  "private_key",
  "privatekey",
  "client_key",
  "bearer",
  "password",
  "token",
  "-----begin",
  ".key",
  "certificate_path",
  "clientcert"
];

test.beforeEach(async ({ page }) => {
  const browserErrors: string[] = [];
  page.on("pageerror", (error) => browserErrors.push(error.message));
  page.on("console", (message) => {
    if (message.type() === "error") {
      const text = message.text();
      const expectedResourceFailure =
        text.includes("Failed to load resource: the server responded with a status of 404") ||
        text.includes("Failed to load resource: the server responded with a status of 503");
      if (!expectedResourceFailure) {
        browserErrors.push(text);
      }
    }
  });
  await page.exposeFunction("__flightDeckBrowserErrors", () => browserErrors);
});

test("fixture mode renders query, schema, and distinct audit evidence", async ({ page }, testInfo) => {
  await page.goto("/");

  await expect(page.getByRole("heading", { name: "GRM flight-deck" })).toBeVisible();
  await expect(page.getByText("Fixture", { exact: true }).first()).toBeVisible();
  await expect(page.getByRole("button", { name: "Data" })).toHaveClass(/active/);
  await page.getByRole("button", { name: "Schema" }).click();
  await expect(page.getByLabel("Query explain and profile")).toHaveCount(0);
  await expect(page.getByRole("heading", { name: "Schema" })).toBeVisible();

  await page.getByRole("button", { name: "Audit" }).click();
  await expect(page.getByText("Service-authored audit evidence and bounded local UI observations.")).toBeVisible();
  await expect(page.getByText("Best-effort audit")).toBeVisible();
  await expect(page.getByText("audit.inspect").first()).toBeVisible();
  await expect(page.getByText("bounded snapshot read")).toBeVisible();
  await expect(page.getByText("local view filter")).toBeVisible();

  await page.screenshot({
    path: testInfo.outputPath("fixture-audit-legibility.png"),
    fullPage: true
  });

  await expectNoBrowserErrors(page);
  await expectNoSecretLikeStorage(page);
});

test("gateway unavailable state remains visible and does not look secured", async ({ page }) => {
  await page.addInitScript((key) => {
    window.localStorage.setItem(
      key,
      JSON.stringify({
        version: 1,
        selectedProfileId: "unavailable-local-gateway",
        profiles: [
          {
            id: "unavailable-local-gateway",
            name: "Unavailable local gateway",
            settings: {
              serviceBaseUrl: "",
              mode: "secured",
              workspace: "flight-deck-demo",
              limit: 50,
              useFixtureData: false
            }
          }
        ]
      })
    );
  }, storeKey);
  await page.route("**/api/**", (route) =>
    route.fulfill({
      status: 503,
      contentType: "text/plain",
      body: "gateway unavailable"
    })
  );

  await page.goto("/");
  await expect(page.getByText("Unavailable local gateway")).toBeVisible();
  const connectButton = page.locator("header").getByRole("button", { name: "Connect", exact: true });
  await expect(connectButton).toBeEnabled();
  await connectButton.click();
  await expect(page.getByText("Connection failed.")).toBeVisible();
  await page.getByRole("button", { name: "Audit" }).click();
  await expect(page.getByText("Audit unavailable")).toBeVisible();
  await expect(page.getByText("unavailable").first()).toBeVisible();
  await expect(page.getByText("Mandatory audit")).toHaveCount(0);

  await expectNoBrowserErrors(page);
  await expectNoSecretLikeStorage(page);
});

async function expectNoBrowserErrors(page: import("@playwright/test").Page) {
  const errors = await page.evaluate(async () => {
    const getter = (window as unknown as { __flightDeckBrowserErrors: () => Promise<string[]> })
      .__flightDeckBrowserErrors;
    return getter();
  });
  expect(errors).toEqual([]);
}

async function expectNoSecretLikeStorage(page: import("@playwright/test").Page) {
  const storage = await page.evaluate(() =>
    Object.fromEntries(
      Array.from({ length: window.localStorage.length }, (_, index) => {
        const key = window.localStorage.key(index) ?? "";
        return [key, window.localStorage.getItem(key) ?? ""];
      })
    )
  );
  const serialized = JSON.stringify(storage).toLowerCase();
  for (const needle of secretLike) {
    expect(serialized).not.toContain(needle);
  }
}
