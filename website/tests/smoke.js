async (page) => {
  const base = page.url().split("#")[0];
  const errors = [];
  const results = [];
  const check = (condition, message) => { if (!condition) throw new Error(message); };
  page.on("pageerror", (error) => errors.push(error.message));
  await page.emulateMedia({ reducedMotion: "no-preference" });
  await page.goto(base);
  await page.locator(".ascii-art.is-ready").waitFor();

  for (const width of [320, 390, 620, 720, 768, 1024, 1440]) {
    await page.setViewportSize({ width, height: 900 });
    check(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), `Overflow at ${width}px`);
  }
  results.push("No page overflow at 320, 390, 620, 720, 768, 1024, and 1440px");

  const frame = () => page.locator("#ascii-wing").evaluate((canvas) => canvas.toDataURL());
  const first = await frame();
  await page.waitForTimeout(200);
  check(first !== await frame(), "ASCII animation did not advance");
  await page.getByRole("button", { name: "Pause animation" }).click();
  const paused = await frame();
  await page.waitForTimeout(200);
  check(paused === await frame(), "Paused animation still advances");
  await page.getByRole("button", { name: "Play animation" }).click();
  await page.waitForTimeout(200);
  check(paused !== await frame(), "Animation did not resume");
  await page.emulateMedia({ reducedMotion: "reduce" });
  const reduced = await frame();
  await page.waitForTimeout(200);
  check(reduced === await frame(), "Reduced-motion preference was ignored");
  check(await page.getByRole("button", { name: "Play animation" }).isVisible(), "Reduced-motion state is not reflected in control");
  results.push("ASCII animates, pauses, resumes, and respects reduced motion");

  await page.emulateMedia({ reducedMotion: "no-preference" });
  for (const name of ["Official", "Blueprint", "Chalkboard", "Retro", "Holographic", "X-ray"]) {
    await page.getByRole("button", { name, exact: true }).click();
    check(await page.locator(".icon-choice[aria-pressed=true]").count() === 1, "Icon selection is not exclusive");
    check(await page.locator("#active-icon").getAttribute("alt") === `${name} Angelly app icon`, `Wrong ${name} preview`);
    await page.waitForFunction(() => { const img = document.querySelector("#active-icon"); return img.complete && img.naturalWidth > 0; });
  }
  const offscreen = await frame();
  await page.waitForTimeout(200);
  check(offscreen === await frame(), "Offscreen animation still advances");
  results.push("All six icon previews load; offscreen animation stops");

  await page.context().grantPermissions(["clipboard-read", "clipboard-write"]);
  await page.getByRole("button", { name: "Copy commands" }).click();
  await page.waitForFunction(() => document.querySelector("#copy-status").textContent.startsWith("Copied."));
  check(await page.evaluate(() => navigator.clipboard.readText()) === await page.locator("#install-commands").textContent(), "Clipboard commands mismatch");
  await page.evaluate(() => { navigator.clipboard.writeText = async () => { throw new Error("Permission denied"); }; });
  await page.getByRole("button", { name: "Copy commands" }).click();
  await page.waitForFunction(() => document.querySelector("#copy-status").textContent.startsWith("Commands selected."));
  check(await page.evaluate(() => getSelection().toString()) === await page.locator("#install-commands").textContent(), "Clipboard fallback did not select commands");
  results.push("Copy writes exact commands; denied clipboard selects text and explains manual copy");

  await page.setViewportSize({ width: 390, height: 844 });
  await page.locator(".mobile-menu summary").click();
  await page.getByRole("navigation", { name: "Mobile navigation" }).getByRole("link", { name: "The terminal" }).click();
  check(page.url().endsWith("#features"), "Mobile navigation target failed");
  await page.goto(base);
  await page.keyboard.press("Tab");
  check(await page.getByRole("link", { name: "Skip to content" }).evaluate((el) => el === document.activeElement), "Skip link not keyboard reachable");
  await page.keyboard.press("Enter");
  check(page.url().endsWith("#main"), "Skip link target failed");
  await page.locator("#icons").scrollIntoViewIfNeeded();
  await page.getByRole("button", { name: "Blueprint", exact: true }).focus();
  await page.keyboard.press("Enter");
  check(await page.locator("#active-icon-name").textContent() === "Blueprint", "Keyboard icon selection failed");
  results.push("Mobile menu, skip link, and keyboard icon selection work");

  const api = "https://api.github.com/repos/adetxt/angelly/releases/latest";
  const dmg = "https://github.com/adetxt/angelly/releases/download/v-test/Angelly-test.dmg";
  await page.route(api, (route) => route.fulfill({ json: { assets: [{ name: "Angelly-test.dmg", browser_download_url: dmg }] } }));
  await page.route(dmg, (route) => route.fulfill({ contentType: "application/octet-stream", body: "download test", headers: { "content-disposition": 'attachment; filename="Angelly-test.dmg"' } }));
  const download = page.waitForEvent("download");
  await page.locator("#macos-download").click();
  check((await download).suggestedFilename() === "Angelly-test.dmg", "DMG selection failed");
  await page.unroute(api);
  await page.route(api, (route) => route.fulfill({ status: 503, body: "Unavailable" }));
  await page.route("https://github.com/adetxt/angelly/releases/latest", (route) => route.fulfill({ contentType: "text/html", body: "<h1>Releases fallback</h1>" }));
  await page.locator("#macos-download").click();
  await page.waitForURL("https://github.com/adetxt/angelly/releases/latest");
  check(await page.getByRole("heading", { name: "Releases fallback" }).isVisible(), "Download fallback failed");
  await page.unrouteAll();
  results.push("Latest DMG is selected; API failure opens releases (mocked, no real download)");

  await page.goto(base);
  await page.evaluate(async () => { for (const img of document.images) { img.loading = "eager"; await img.decode(); } });
  check(await page.evaluate(() => [...document.images].every((img) => img.complete && img.naturalWidth > 0)), "Broken image");
  check(await page.evaluate(() => [...document.querySelectorAll('a[href^="#"]')].every((a) => document.getElementById(a.hash.slice(1)))), "Broken anchor");
  check(errors.length === 0, errors.join("\n"));
  results.push("Images and anchor targets resolve; no JavaScript errors");
  return results;
}
