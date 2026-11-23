const iconChoices = document.querySelectorAll(".icon-choice");
const activeIcon = document.querySelector("#active-icon");
const activeIconName = document.querySelector("#active-icon-name");

iconChoices.forEach((choice) => {
  choice.addEventListener("click", () => {
    activeIcon.src = choice.dataset.src;
    activeIcon.alt = `${choice.dataset.name} Angelly app icon`;
    activeIconName.textContent = choice.dataset.name;
    iconChoices.forEach((item) => item.setAttribute("aria-pressed", String(item === choice)));
  });
});

const copyButton = document.querySelector("#copy-commands");
const installCommands = document.querySelector("#install-commands");
const copyStatus = document.querySelector("#copy-status");

copyButton.addEventListener("click", async () => {
  try {
    await navigator.clipboard.writeText(installCommands.textContent);
    copyStatus.textContent = "Copied. Paste into your terminal to install.";
  } catch {
    const selection = window.getSelection();
    const range = document.createRange();
    range.selectNodeContents(installCommands);
    selection.removeAllRanges();
    selection.addRange(range);
    copyStatus.textContent = "Commands selected. Press ⌘C or Ctrl+C to copy.";
  }
});

const downloadLink = document.querySelector("#macos-download");
const downloadStatus = document.querySelector("#download-status");
let findingRelease = false;

downloadLink.addEventListener("click", async (event) => {
  // Keep the release page available for new tabs and without JavaScript.
  if (event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return;
  event.preventDefault();
  if (findingRelease) return;
  findingRelease = true;
  downloadLink.setAttribute("aria-busy", "true");
  downloadStatus.textContent = "Finding the latest macOS release…";
  const controller = new AbortController();
  const timeout = window.setTimeout(() => controller.abort(), 8000);
  try {
    const response = await fetch("https://api.github.com/repos/adetxt/angelly/releases/latest", {
      headers: { Accept: "application/vnd.github+json" },
      signal: controller.signal,
    });
    if (!response.ok) throw new Error("Release lookup failed");
    const release = await response.json();
    const dmg = release.assets?.find((asset) => /^Angelly-.+\.dmg$/i.test(asset.name));
    const url = new URL(dmg?.browser_download_url);
    if (url.origin !== "https://github.com" || !url.pathname.startsWith("/adetxt/angelly/releases/download/")) {
      throw new Error("No Angelly macOS download found");
    }
    downloadStatus.textContent = "Your download is starting.";
    window.location.assign(url.href);
  } catch {
    downloadStatus.textContent = "Opening the releases page. Choose the latest macOS DMG.";
    window.location.assign(downloadLink.href);
  } finally {
    window.clearTimeout(timeout);
    findingRelease = false;
    downloadLink.removeAttribute("aria-busy");
  }
});

function initializeAsciiWing() {
  const canvas = document.querySelector("#ascii-wing");
  const context = canvas.getContext("2d");
  if (!context) return;
  const figure = document.querySelector(".ascii-art");
  const toggle = document.querySelector("#motion-toggle");
  const reducedMotion = window.matchMedia("(prefers-reduced-motion: reduce)");
  const mask = new Image();

  mask.addEventListener("load", () => {
    const columns = 80;
    const rows = 52;
    const sample = document.createElement("canvas");
    sample.width = columns;
    sample.height = rows;
    const sampler = sample.getContext("2d", { willReadFrequently: true });
    if (!sampler) return;
    // Crop to the original wing-and-prompt silhouette, excluding the mask's margin.
    sampler.drawImage(mask, 270, 215, 500, 540, 0, 0, columns, rows);
    const pixels = sampler.getImageData(0, 0, columns, rows).data;
    const points = [];
    for (let y = 0; y < rows; y++) {
      for (let x = 0; x < columns; x++) {
        if (pixels[(y * columns + x) * 4 + 3] > 100) points.push({ x, y });
      }
    }

    const characters = ".:+*o%@#";
    let paused = reducedMotion.matches;
    let visible = false;
    let frame = 0;
    let time = 0;
    let lastTimestamp = 0;
    let lastDraw = 0;
    const pixelRatio = Math.min(window.devicePixelRatio || 1, 2);
    canvas.width = 640 * pixelRatio;
    canvas.height = 600 * pixelRatio;
    context.scale(pixelRatio, pixelRatio);
    context.font = '10px "SFMono-Regular", Consolas, "Liberation Mono", monospace';
    context.textAlign = "center";
    context.textBaseline = "middle";

    function draw() {
      context.clearRect(0, 0, 640, 600);
      const angle = Math.sin(time * .32) * .27;
      const cos = Math.cos(angle);
      const sin = Math.sin(angle);
      for (const { x, y } of points) {
        const wave = Math.sin(x * .11 + y * .09 - time * .8);
        const depth = wave * 12;
        const px = (x - columns / 2) * 6;
        const py = (y - rows / 2) * 9.1;
        const scale = 760 / (760 - px * sin - depth);
        const light = (Math.sin(x * .07 - y * .08 + time * .6) + 1) / 2;
        const index = Math.min(characters.length - 1, Math.floor((.25 + light * .74) * characters.length));
        context.fillStyle = light > .68 ? "#8c4168" : light > .3 ? "#683551" : "#b57d9e";
        context.fillText(characters[index], 320 + (px * cos + depth * sin) * scale, 290 + (py + wave * 3) * scale);
      }
    }

    function tick(timestamp) {
      frame = 0;
      if (lastTimestamp) time += Math.min(timestamp - lastTimestamp, 100) / 1000;
      lastTimestamp = timestamp;
      if (timestamp - lastDraw > 65) {
        draw();
        lastDraw = timestamp;
      }
      frame = window.requestAnimationFrame(tick);
    }

    function updatePlayback() {
      window.cancelAnimationFrame(frame);
      frame = 0;
      lastTimestamp = 0;
      if (!paused && visible && !document.hidden) frame = window.requestAnimationFrame(tick);
      toggle.textContent = paused ? "Play animation" : "Pause animation";
    }

    toggle.addEventListener("click", () => {
      paused = !paused;
      updatePlayback();
    });
    reducedMotion.addEventListener("change", () => {
      paused = reducedMotion.matches;
      updatePlayback();
    });
    document.addEventListener("visibilitychange", updatePlayback);
    new IntersectionObserver(([entry]) => {
      visible = entry.isIntersecting;
      updatePlayback();
    }).observe(canvas);

    draw();
    figure.classList.add("is-ready");
    toggle.hidden = false;
    updatePlayback();
  });
  mask.src = "assets/wing-mask.png";
}

initializeAsciiWing();
