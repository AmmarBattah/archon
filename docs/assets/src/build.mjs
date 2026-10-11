// Builds ../archon_graphic.svg from archon_graphic.src.svg by outlining every
// <text> into shared glyph <path>s, so the published SVG needs no fonts at all.
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import assert from "node:assert/strict";
import opentype from "opentype.js";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const SOURCE = path.join(HERE, "archon_graphic.src.svg");
const OUTPUT = path.join(HERE, "..", "archon_graphic.svg");

const FONT_FILES = {
  sans: "FiraSans-Regular.ttf",
  "sans-md": "FiraSans-Medium.ttf",
  "sans-sb": "FiraSans-SemiBold.ttf",
  "sans-b": "FiraSans-Bold.ttf",
  "sans-black": "FiraSans-Black.ttf",
  mono: "FiraMono-Regular.ttf",
  "mono-md": "FiraMono-Medium.ttf",
  hand: "Caveat[wght].ttf",
};

const ENTITIES = { "&amp;": "&", "&lt;": "<", "&gt;": ">", "&quot;": '"', "&apos;": "'" };

function loadFonts() {
  const fonts = {};
  for (const [cls, file] of Object.entries(FONT_FILES)) {
    const fontPath = path.join(HERE, "fonts", file);
    assert.ok(fs.existsSync(fontPath), `missing font ${fontPath}; run ./fetch-fonts.sh`);
    fonts[cls] = opentype.loadSync(fontPath);
    assert.ok(fonts[cls].unitsPerEm > 0, `font ${file} has no unitsPerEm`);
  }
  return fonts;
}

function parseAttrs(source) {
  const attrs = {};
  for (const match of source.matchAll(/([\w:-]+)="([^"]*)"/g)) attrs[match[1]] = match[2];
  return attrs;
}

function decodeEntities(text) {
  const decoded = text
    .replace(/&#x([0-9a-f]+);/gi, (_, hex) => String.fromCodePoint(parseInt(hex, 16)))
    .replace(/&(amp|lt|gt|quot|apos);/g, (entity) => ENTITIES[entity]);
  assert.ok(!/&\w+;/.test(decoded), `unknown entity in "${text}"`);
  return decoded;
}

// Splits a <text> body into styled runs; tspans only override class, fill and font-size.
function parseRuns(body, base) {
  const runs = [];
  for (const match of body.matchAll(/<tspan\b([^>]*)>([^<]*)<\/tspan>|([^<]+)/g)) {
    const own = match[1] === undefined ? {} : parseAttrs(match[1]);
    const text = decodeEntities(match[2] ?? match[3]);
    runs.push({
      text,
      cls: own.class ?? base.class,
      fill: own.fill ?? base.fill,
      size: Number(own["font-size"] ?? base["font-size"]),
    });
  }
  assert.ok(runs.length > 0, `empty <text> element: ${body}`);
  return runs;
}

function layoutRun(run, fonts, spacing, penX) {
  const font = fonts[run.cls];
  assert.ok(font, `unknown text class "${run.cls}"`);
  assert.ok(run.size > 0 && run.size < 200, `bad font-size ${run.size}`);
  const scale = run.size / font.unitsPerEm;
  const glyphs = font.stringToGlyphs(run.text);
  const placed = [];
  let x = penX;
  glyphs.forEach((glyph, i) => {
    const char = [...run.text][i];
    assert.ok(glyph.index !== 0 || char === " ", `glyph for "${char}" missing in ${run.cls}`);
    placed.push({ glyph, x });
    const kern = i + 1 < glyphs.length ? font.getKerningValue(glyph, glyphs[i + 1]) : 0;
    x += (glyph.advanceWidth + kern) * scale + spacing;
  });
  return { placed, font, endX: x };
}

function round(value, digits = 1) {
  assert.ok(Number.isFinite(value), `non-finite coordinate ${value}`);
  return Number(value.toFixed(digits));
}

// Each distinct glyph is emitted once, in font units, and referenced by <use>.
function createGlyphTable() {
  const ids = new Map();
  const defs = [];
  return {
    idFor(cls, glyph, unitsPerEm) {
      const key = `${cls}:${glyph.index}`;
      if (!ids.has(key)) {
        const id = `gl${ids.size}`;
        ids.set(key, id);
        const d = glyph.getPath(0, 0, unitsPerEm).toPathData(0);
        defs.push(`<path id="${id}" d="${d}"/>`);
      }
      return ids.get(key);
    },
    defs: () => defs.join("\n"),
    size: () => ids.size,
  };
}

function outlineText(attrs, body, fonts, glyphs) {
  const x = Number(attrs.x ?? 0);
  const y = Number(attrs.y ?? 0);
  assert.ok(Number.isFinite(x) && Number.isFinite(y), `bad text position ${attrs.x},${attrs.y}`);
  const spacing = Number(attrs["letter-spacing"] ?? 0);
  const runs = parseRuns(body, attrs);

  let penX = 0;
  const laidOut = runs.map((run) => {
    const result = layoutRun(run, fonts, spacing, penX);
    penX = result.endX;
    return { run, ...result };
  });
  const width = penX - spacing;
  const anchor = attrs["text-anchor"] ?? "start";
  const shift = { start: 0, middle: -width / 2, end: -width }[anchor];
  assert.ok(shift !== undefined, `unknown text-anchor "${anchor}"`);

  const paths = laidOut.map(({ run, placed, font }) => {
    const scale = run.size / font.unitsPerEm;
    const uses = placed
      .filter(({ glyph }) => glyph.path.commands.length > 0)
      .map(({ glyph, x: gx }) => `<use href="#${glyphs.idFor(run.cls, glyph, font.unitsPerEm)}" x="${round(gx / scale)}"/>`)
      .join("");
    const origin = `translate(${round(x + shift)} ${round(y)}) scale(${round(scale, 5)})`;
    return `<g fill="${run.fill}" transform="${origin}">${uses}</g>`;
  });
  const wrapper = ["transform", "opacity"]
    .filter((name) => attrs[name] !== undefined)
    .map((name) => ` ${name}="${attrs[name]}"`)
    .join("");
  return paths.length === 1 && !wrapper ? paths[0] : `<g${wrapper}>${paths.join("")}</g>`;
}

function build() {
  const fonts = loadFonts();
  const source = fs.readFileSync(SOURCE, "utf8");
  assert.ok(source.includes("<svg"), "source is not an SVG document");

  let count = 0;
  const glyphs = createGlyphTable();
  const outlined = source
    .replace(/\s*<style id="source-fonts">[\s\S]*?<\/style>/, "")
    .replace(/<text\b([^>]*)>([\s\S]*?)<\/text>/g, (_, attrSource, body) => {
      count += 1;
      return outlineText(parseAttrs(attrSource), body, fonts, glyphs);
    })
    .replace(/<!--[\s\S]*?-->\n?/g, "");

  assert.ok(count > 0 && glyphs.size() > 0, "no <text> elements were outlined");
  assert.equal(outlined.split("</defs>").length, 2, "source must have exactly one <defs>");
  assert.ok(!/<text\b|url\(['"]?fonts\//.test(outlined), "unconverted text or font reference remains");
  fs.writeFileSync(OUTPUT, outlined.replace("</defs>", `${glyphs.defs()}\n  </defs>`));
  console.log(`outlined ${count} text elements (${glyphs.size()} glyphs) -> ${path.relative(process.cwd(), OUTPUT)}`);
}

build();
