# Angelly landing page

A static site with no build step or runtime dependencies. From the repository root:

```sh
python3 -m http.server 8000 --bind 127.0.0.1
```

Open <http://127.0.0.1:8000/website/>. Alternatively, serve `website/` directly;
all assets live inside it so the same files work with the existing Firebase setup.

## Design

The layout takes its spacious typography and product focus from [Warp](https://www.warp.dev/).
The animated character art takes its cue from [Ghostty](https://ghostty.org/), using
Angelly's existing wing-and-prompt mask rather than Ghostty's mascot.

- Blush `#fff6fa` and plum `#211922` come from the Angelly Light and Dark themes.
  Rose accents and the lilac Official app icon connect the page to the product.
- System sans-serif type keeps prose readable and feels at home on macOS;
  monospace is reserved for command-line details and character art.
- The split introduction gives the wing space; the real product screenshot,
  compact feature descriptions, dark engine section, and interactive icon picker
  give each kind of content its own layout.
- Energy 2 / rhythm 2 / motion 2: the wing provides the motion and identity.
  Other content stays still. Whitespace separates the introduction, product,
  customization, and installation rather than adding decorative sections.

`script.js` samples the existing mask into a character field and animates its
shading and perspective at roughly 15 frames per second. Rendering stops when
paused, offscreen, or in a hidden tab. Reduced motion starts with a still frame;
the visitor can explicitly play it. The Official icon is the fallback when
JavaScript or canvas is unavailable.

The icon picker uses the original six previews. Homebrew commands remain
selectable if clipboard access is denied. The download link resolves the latest
Angelly DMG, with an eight-second timeout and the GitHub releases page as fallback.
Without JavaScript, the release link and commands still work.

## Browser smoke check

With the local server running, use the Playwright CLI (no project install needed):

```sh
npx --package @playwright/cli playwright-cli -s=angelly-smoke open http://127.0.0.1:8000/website/
npx --package @playwright/cli playwright-cli -s=angelly-smoke run-code --filename=website/tests/smoke.js
npx --package @playwright/cli playwright-cli -s=angelly-smoke close
```

The check covers responsive overflow, animation playback and reduced motion,
all icon previews, clipboard success and denial, mobile navigation, keyboard
access, image and anchor targets, and download success and fallback. Release
responses and the download are mocked, so it does not download a real app.

## Verification

- Hard gate PASS: seven widths from 320 to 1440px have no horizontal overflow;
  text contrast is at least 4.89:1 across 13 color pairs; asset and anchor checks
  pass. Browser checks cover the controls, keyboard use, and failure states.
  Copy uses existing product features, with no testimonials or invented metrics.
- Purpose gate PASS: the palette comes from the shipped themes, the ASCII art
  uses the original wing, and the screenshot shows the actual app. Typography,
  layout, and motion choices are explained above. No decorative grids, glows,
  figure numbering, or card effects were added.
- Liveliness PASS: energy 2 / rhythm 2 / motion 2. The animated wing is the hero's
  focal point; rose accents, the wing mark, and space between content sections
  carry the identity. Product, engine, icon, and install sections vary by purpose.
- Craftsmanship PASS: browser smoke checks pass, all six icon images load, and
  clipboard success and denial, reduced motion, offscreen pause, and release API
  failure are exercised. A separate JavaScript-disabled check confirms the icon,
  release link, and selectable commands remain available. Desktop, tablet, and
  mobile screenshots were inspected. JavaScript syntax and Git whitespace checks
  pass. Safari and Firefox were not tested; nothing was deployed.
