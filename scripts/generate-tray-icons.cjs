#!/usr/bin/env node

/**
 * Regenerate tray icons from the current Lingxi logo:
 *   desktop/src/assets/tray-template.png        (16px,  macOS template silhouette)
 *   desktop/src/assets/tray-template@2x.png     (32px,  macOS template silhouette, retina)
 *   desktop/src/assets/tray-dev-template.png    (16px,  dev variant with "D" badge)
 *   desktop/src/assets/tray-dev-template@2x.png (32px,  dev variant, retina)
 *   desktop/src/assets/tray.ico                 (Windows tray, mini app icon)
 *   desktop/src/assets/tray-dev.ico             (Windows tray, dev variant with "D" badge)
 *
 * The silhouette mask comes from desktop/src/assets/Lingxi.png (white background,
 * dark outline): flood-fill the near-white background from the borders, invert.
 * The .ico layers come from desktop/src/icon.png with the same rounded-rect
 * coverage as scripts/generate-windows-icon.cjs.
 */

const fs = require("fs");
const path = require("path");
const { PNG } = require("pngjs");

const ROOT = path.resolve(__dirname, "..");
const SILHOUETTE_SOURCE = path.join(ROOT, "desktop", "src", "assets", "Lingxi.png");
const COLOR_SOURCE = path.join(ROOT, "desktop", "src", "icon.png");
const OUT_DIR = path.join(ROOT, "desktop", "src", "assets");

const ICO_SIZES = [256, 128, 64, 48, 32, 24, 16];
const CORNER_RADIUS_RATIO = 0.225;
const BACKGROUND_MIN_CHANNEL = 235;

// Badge geometry in normalized [0,1] coordinates (bottom-right corner).
const BADGE_CENTER = 0.7;
const BADGE_RADIUS = 0.3;
const BADGE_RING_INNER = 0.22;

// 4x5 "D" glyph, filled cells marked with "#".
const GLYPH_D = ["###.", "#..#", "#..#", "#..#", "###."];
const GLYPH_BOX_WIDTH = 0.28;
const GLYPH_BOX_HEIGHT = 0.34;

function clamp(value, min, max) {
  return Math.max(min, Math.min(max, value));
}

function alphaForRoundedRect(x, y, size, radius) {
  const px = x + 0.5;
  const py = y + 0.5;
  const half = size / 2;
  const qx = Math.abs(px - half) - (half - radius);
  const qy = Math.abs(py - half) - (half - radius);
  const outsideX = Math.max(qx, 0);
  const outsideY = Math.max(qy, 0);
  const outside = Math.hypot(outsideX, outsideY);
  const inside = Math.min(Math.max(qx, qy), 0);
  const signedDistance = outside + inside - radius;
  return clamp(0.5 - signedDistance, 0, 1);
}

function sampleArea(source, left, top, right, bottom) {
  const x0 = Math.floor(left);
  const x1 = Math.ceil(right);
  const y0 = Math.floor(top);
  const y1 = Math.ceil(bottom);
  let total = 0;
  let r = 0;
  let g = 0;
  let b = 0;
  let a = 0;

  for (let sy = y0; sy < y1; sy++) {
    if (sy < 0 || sy >= source.height) continue;
    const oy = Math.max(0, Math.min(bottom, sy + 1) - Math.max(top, sy));
    if (oy <= 0) continue;

    for (let sx = x0; sx < x1; sx++) {
      if (sx < 0 || sx >= source.width) continue;
      const ox = Math.max(0, Math.min(right, sx + 1) - Math.max(left, sx));
      const weight = ox * oy;
      const index = (sy * source.width + sx) * 4;
      const alpha = source.data[index + 3] / 255;
      const weightedAlpha = alpha * weight;
      r += source.data[index] * weightedAlpha;
      g += source.data[index + 1] * weightedAlpha;
      b += source.data[index + 2] * weightedAlpha;
      a += weightedAlpha;
      total += weight;
    }
  }

  if (total === 0 || a === 0) return [0, 0, 0, 0];

  return [
    Math.round(r / a),
    Math.round(g / a),
    Math.round(b / a),
    Math.round((a / total) * 255),
  ];
}

/**
 * Build a creature mask (1 = bird silhouette, 0 = background) from the source.
 *
 * Preferred path when the source ships a real alpha channel (transparent-background
 * artwork): the silhouette is simply the opaque pixels, so mask = alpha > threshold.
 * Falls back to the white-background flood-fill when the source has no usable alpha
 * (legacy opaque artwork).
 */
function buildCreatureMask(png) {
  const { width, height, data, colorType } = png;
  const hasAlpha = colorType === 6;
  // Use the alpha channel whenever the source is RGBA. A transparent-background
  // artwork has alpha=0 on the backdrop and alpha=255 inside the bird, so the
  // opaque pixels form the silhouette directly. (Fully-opaque RGBA images — e.g.
  // a legacy white-background logo — also have alpha=255 everywhere; in that case
  // the flood-fill path below still applies because there are no transparent
  // background pixels to separate the subject.)
  let hasTransparent = false;
  if (hasAlpha) {
    for (let i = 3; i < data.length; i += 4) {
      if (data[i] < 255) { hasTransparent = true; break; }
    }
  }
  if (hasAlpha && hasTransparent) {
    const mask = new Float32Array(width * height);
    let creature = 0;
    for (let i = 0; i < width * height; i++) {
      const a = data[i * 4 + 3] / 255;
      mask[i] = a > 0.4 ? 1 : 0;
      if (mask[i]) creature++;
    }
    const coverage = creature / mask.length;
    if (coverage < 0.05 || coverage > 0.95) {
      throw new Error(
        `Creature alpha-mask coverage looks wrong (${(coverage * 100).toFixed(1)}%); ` +
          "the alpha channel may be absent or the threshold is off.",
      );
    }
    return mask;
  }

  const passable = (x, y) => {
    const i = (y * width + x) * 4;
    return (
      data[i] >= BACKGROUND_MIN_CHANNEL &&
      data[i + 1] >= BACKGROUND_MIN_CHANNEL &&
      data[i + 2] >= BACKGROUND_MIN_CHANNEL
    );
  };

  const background = new Uint8Array(width * height);
  const queue = [];
  for (let x = 0; x < width; x++) {
    queue.push([x, 0], [x, height - 1]);
  }
  for (let y = 0; y < height; y++) {
    queue.push([0, y], [width - 1, y]);
  }

  while (queue.length > 0) {
    const [x, y] = queue.pop();
    if (x < 0 || y < 0 || x >= width || y >= height) continue;
    const idx = y * width + x;
    if (background[idx] || !passable(x, y)) continue;
    background[idx] = 1;
    queue.push([x + 1, y], [x - 1, y], [x, y + 1], [x, y - 1]);
  }

  const mask = new Float32Array(width * height);
  let creaturePixels = 0;
  for (let i = 0; i < mask.length; i++) {
    mask[i] = background[i] ? 0 : 1;
    if (mask[i] === 1) creaturePixels++;
  }
  const coverage = creaturePixels / mask.length;
  if (coverage < 0.05 || coverage > 0.95) {
    throw new Error(
      `Creature mask looks wrong (coverage ${(coverage * 100).toFixed(1)}%); ` +
        "the outline is probably not closed or the threshold is off.",
    );
  }
  return mask;
}

/** Area-average the full-res mask down to `size`, giving anti-aliased alpha. */
function downsampleMask(mask, srcWidth, srcHeight, size) {
  const out = new Float32Array(size * size);
  const cellW = srcWidth / size;
  const cellH = srcHeight / size;
  for (let y = 0; y < size; y++) {
    for (let x = 0; x < size; x++) {
      const left = x * cellW;
      const top = y * cellH;
      const right = (x + 1) * cellW;
      const bottom = (y + 1) * cellH;
      let sum = 0;
      let total = 0;
      for (let sy = Math.floor(top); sy < Math.ceil(bottom); sy++) {
        const oy = Math.max(0, Math.min(bottom, sy + 1) - Math.max(top, sy));
        if (oy <= 0) continue;
        for (let sx = Math.floor(left); sx < Math.ceil(right); sx++) {
          const ox = Math.max(0, Math.min(right, sx + 1) - Math.max(left, sx));
          if (ox <= 0) continue;
          sum += mask[sy * srcWidth + sx] * ox * oy;
          total += ox * oy;
        }
      }
      out[y * size + x] = total > 0 ? sum / total : 0;
    }
  }
  return out;
}

/** True when normalized pixel (nx, ny) falls on a filled cell of the "D" glyph. */
function glyphDFilled(nx, ny) {
  const left = BADGE_CENTER - GLYPH_BOX_WIDTH / 2;
  const top = BADGE_CENTER - GLYPH_BOX_HEIGHT / 2;
  if (nx < left || nx >= left + GLYPH_BOX_WIDTH || ny < top || ny >= top + GLYPH_BOX_HEIGHT) {
    return false;
  }
  const col = Math.min(GLYPH_D[0].length - 1, Math.floor(((nx - left) / GLYPH_BOX_WIDTH) * GLYPH_D[0].length));
  const row = Math.min(GLYPH_D.length - 1, Math.floor(((ny - top) / GLYPH_BOX_HEIGHT) * GLYPH_D.length));
  return GLYPH_D[row][col] === "#";
}

/** Render a macOS template icon: black pixels, alpha from the creature mask. */
function renderTemplate(mask, srcWidth, srcHeight, size, { devBadge }) {
  const alpha = downsampleMask(mask, srcWidth, srcHeight, size);
  const png = new PNG({ width: size, height: size, colorType: 6 });

  for (let y = 0; y < size; y++) {
    for (let x = 0; x < size; x++) {
      const i = (y * size + x) * 4;
      let a = Math.round(alpha[y * size + x] * 255);
      if (devBadge) {
        const nx = (x + 0.5) / size;
        const ny = (y + 0.5) / size;
        const dist = Math.hypot(nx - BADGE_CENTER, ny - BADGE_CENTER);
        if (dist <= BADGE_RADIUS) {
          // Badge dot with the "D" knocked out (transparent) for legibility.
          a = glyphDFilled(nx, ny) ? 0 : 255;
        }
      }
      png.data[i] = 0;
      png.data[i + 1] = 0;
      png.data[i + 2] = 0;
      png.data[i + 3] = a;
    }
  }
  return PNG.sync.write(png, { colorType: 6 });
}

/** Render one .ico layer: mini app icon, optionally with an orange "D" badge. */
function renderIcoLayer(source, size, { devBadge }) {
  const png = new PNG({ width: size, height: size, colorType: 6 });
  const sourceSize = Math.min(source.width, source.height);
  const sourceX = (source.width - sourceSize) / 2;
  const sourceY = (source.height - sourceSize) / 2;
  const radius = size * CORNER_RADIUS_RATIO;

  for (let y = 0; y < size; y++) {
    for (let x = 0; x < size; x++) {
      const left = sourceX + (x / size) * sourceSize;
      const top = sourceY + (y / size) * sourceSize;
      const right = sourceX + ((x + 1) / size) * sourceSize;
      const bottom = sourceY + ((y + 1) / size) * sourceSize;
      const [r, g, b, a] = sampleArea(source, left, top, right, bottom);
      const coverage = alphaForRoundedRect(x, y, size, radius);
      const index = (y * size + x) * 4;

      let pr = r;
      let pg = g;
      let pb = b;
      let pa = Math.round(a * coverage);

      if (devBadge) {
        const nx = (x + 0.5) / size;
        const ny = (y + 0.5) / size;
        const px = nx * size;
        const py = ny * size;
        const dist = Math.hypot(px - BADGE_CENTER * size, py - BADGE_CENTER * size);
        const outer = BADGE_RADIUS * size;
        const inner = BADGE_RING_INNER * size;
        if (dist <= outer + 0.5) {
          const badgeAlpha = clamp(outer + 0.5 - dist, 0, 1);
          let br;
          let bg;
          let bb;
          if (glyphDFilled(nx, ny)) {
            br = 255; bg = 255; bb = 255; // white "D"
          } else if (dist > inner) {
            br = 255; bg = 255; bb = 255; // white ring
          } else {
            br = 255; bg = 140; bb = 0; // orange fill
          }
          pr = Math.round(br * badgeAlpha + pr * (1 - badgeAlpha));
          pg = Math.round(bg * badgeAlpha + pg * (1 - badgeAlpha));
          pb = Math.round(bb * badgeAlpha + pb * (1 - badgeAlpha));
          pa = Math.max(pa, Math.round(255 * badgeAlpha));
        }
      }

      png.data[index] = pr;
      png.data[index + 1] = pg;
      png.data[index + 2] = pb;
      png.data[index + 3] = pa;
    }
  }

  return PNG.sync.write(png, { colorType: 6 });
}

function createIco(layers) {
  const headerSize = 6;
  const entrySize = 16;
  const directorySize = headerSize + layers.length * entrySize;
  const imageSize = layers.reduce((sum, layer) => sum + layer.data.length, 0);
  const out = Buffer.alloc(directorySize + imageSize);

  out.writeUInt16LE(0, 0);
  out.writeUInt16LE(1, 2);
  out.writeUInt16LE(layers.length, 4);

  let imageOffset = directorySize;
  for (let i = 0; i < layers.length; i++) {
    const layer = layers[i];
    const entryOffset = headerSize + i * entrySize;
    out[entryOffset] = layer.size === 256 ? 0 : layer.size;
    out[entryOffset + 1] = layer.size === 256 ? 0 : layer.size;
    out[entryOffset + 2] = 0;
    out[entryOffset + 3] = 0;
    out.writeUInt16LE(1, entryOffset + 4);
    out.writeUInt16LE(32, entryOffset + 6);
    out.writeUInt32LE(layer.data.length, entryOffset + 8);
    out.writeUInt32LE(imageOffset, entryOffset + 12);
    layer.data.copy(out, imageOffset);
    imageOffset += layer.data.length;
  }

  return out;
}

function writeOut(fileName, data) {
  const target = path.join(OUT_DIR, fileName);
  fs.writeFileSync(target, data);
  console.log(`wrote ${path.relative(ROOT, target)}`);
}

function main() {
  const silhouetteSource = PNG.sync.read(fs.readFileSync(SILHOUETTE_SOURCE));
  const colorSource = PNG.sync.read(fs.readFileSync(COLOR_SOURCE));

  const mask = buildCreatureMask(silhouetteSource);

  writeOut("tray-template.png", renderTemplate(mask, silhouetteSource.width, silhouetteSource.height, 16, { devBadge: false }));
  writeOut("tray-template@2x.png", renderTemplate(mask, silhouetteSource.width, silhouetteSource.height, 32, { devBadge: false }));
  writeOut("tray-dev-template.png", renderTemplate(mask, silhouetteSource.width, silhouetteSource.height, 16, { devBadge: true }));
  writeOut("tray-dev-template@2x.png", renderTemplate(mask, silhouetteSource.width, silhouetteSource.height, 32, { devBadge: true }));

  const normalLayers = ICO_SIZES.map((size) => ({ size, data: renderIcoLayer(colorSource, size, { devBadge: false }) }));
  writeOut("tray.ico", createIco(normalLayers));
  const devLayers = ICO_SIZES.map((size) => ({ size, data: renderIcoLayer(colorSource, size, { devBadge: true }) }));
  writeOut("tray-dev.ico", createIco(devLayers));
}

main();
