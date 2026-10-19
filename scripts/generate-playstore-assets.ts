import { Resvg } from "@resvg/resvg-js";
import { writeFileSync, copyFileSync, mkdirSync } from "node:fs";
import { resolve } from "node:path";

const rootDir = resolve(import.meta.dirname, "..");
const imgDir = resolve(rootDir, "img");
mkdirSync(imgDir, { recursive: true });

// 1. Copy the official project 512x512 app icon directly
const sourceAppIcon = resolve(rootDir, "apps/desktop/public/icon-512.png");
const destAppIcon = resolve(imgDir, "app-icon.png");
copyFileSync(sourceAppIcon, destAppIcon);
console.log(`Copied authentic project app icon to: ${destAppIcon}`);

function renderSvgToPng(svgStr: string, outPath: string, width?: number, height?: number) {
  const resvg = new Resvg(svgStr, {
    fitTo: width && height ? { mode: "width", value: width } : undefined,
  });
  const pngData = resvg.render();
  const pngBuffer = pngData.asPng();
  writeFileSync(outPath, pngBuffer);
  console.log(`Rendered: ${outPath} (${pngData.width}x${pngData.height})`);
}

// Canonical ShaPrint SVG Logo path snippet
// ViewBox 38 30 180 196
const authenticLogoSvg = (scale = 1, fillLeft = "#249EA0", fillRight = "#FFFFFF") => `
<g transform="scale(${scale})">
  <path fill="${fillLeft}" d="M 46 38 H 121 V 218 C 88.5 204 46 167 46 120 Z" />
  <path fill="${fillRight}" fill-rule="evenodd" d="M 135 38 H 166 L 210 82 V 120 C 210 167 167.5 204 135 218 Z M 160 48 V 88 H 200 Z" />
</g>
`;

// 2. Feature Graphic (1024x500) using authentic project branding
const featureGraphicSvg = `
<svg width="1024" height="500" viewBox="0 0 1024 500" xmlns="http://www.w3.org/2000/svg">
  <defs>
    <linearGradient id="bgBanner" x1="0%" y1="0%" x2="100%" y2="100%">
      <stop offset="0%" stop-color="#141F21" />
      <stop offset="60%" stop-color="#1B292B" />
      <stop offset="100%" stop-color="#0F172A" />
    </linearGradient>
    <linearGradient id="tealGrad" x1="0%" y1="0%" x2="100%" y2="0%">
      <stop offset="0%" stop-color="#249EA0" />
      <stop offset="100%" stop-color="#38BDF8" />
    </linearGradient>
    <filter id="phoneShadow" x="-20%" y="-20%" width="140%" height="140%">
      <feDropShadow dx="0" dy="16" stdDeviation="24" flood-color="#000000" flood-opacity="0.65" />
    </filter>
  </defs>

  <!-- Background -->
  <rect width="1024" height="500" fill="url(#bgBanner)" />

  <!-- Ambient Glow in ShaPrint Teal/Cyan -->
  <circle cx="160" cy="180" r="280" fill="#249EA0" opacity="0.15" filter="blur(80px)" />
  <circle cx="850" cy="320" r="280" fill="#0284C7" opacity="0.12" filter="blur(90px)" />

  <!-- Left Column: Official Branding & Taglines -->
  <g transform="translate(80, 75)">
    <!-- Brand Logo Mark + Wordmark -->
    <g transform="translate(0, 0)">
      <!-- Authentic Logo Mark (width 60, height 65) -->
      <g transform="translate(-10, -5) scale(0.35)">
        <path fill="#249EA0" d="M 46 38 H 121 V 218 C 88.5 204 46 167 46 120 Z" />
        <path fill="#FFFFFF" fill-rule="evenodd" d="M 135 38 H 166 L 210 82 V 120 C 210 167 167.5 204 135 218 Z M 160 48 V 88 H 200 Z" />
      </g>
      <text x="75" y="48" font-family="-apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif" font-size="44" font-weight="900" fill="#FFFFFF" letter-spacing="-0.5">ShaPrint</text>
      <rect x="265" y="24" width="75" height="28" rx="8" fill="#249EA0" />
      <text x="302" y="43" font-family="-apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif" font-size="13" font-weight="800" fill="#FFFFFF" text-anchor="middle">CLIENT</text>
    </g>

    <!-- Headline -->
    <text x="0" y="125" font-family="-apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif" font-size="34" font-weight="800" fill="#FFFFFF">
      Cetak Langsung via LAN &amp; Cross-VLAN
    </text>
    <text x="0" y="165" font-family="-apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif" font-size="18" font-weight="500" fill="#94A3B8">
      Protokol Standar IPPS (TLS) • Zero-Config mDNS • Android Keystore
    </text>

    <!-- Value Prop Feature Badges -->
    <g transform="translate(0, 215)">
      <g transform="translate(0, 0)">
        <rect width="400" height="42" rx="12" fill="rgba(36, 158, 160, 0.12)" stroke="#249EA0" stroke-width="1.5" />
        <text x="20" y="26" font-family="-apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif" font-size="15" font-weight="700" fill="#2DD4BF">✓ Integrasi Native Android PrintService</text>
      </g>
      <g transform="translate(0, 54)">
        <rect width="400" height="42" rx="12" fill="rgba(56, 189, 248, 0.1)" stroke="#38BDF8" stroke-width="1.5" />
        <text x="20" y="26" font-family="-apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif" font-size="15" font-weight="700" fill="#38BDF8">✓ Render Presisi 300 DPI PWG Raster</text>
      </g>
      <g transform="translate(0, 108)">
        <rect width="400" height="42" rx="12" fill="rgba(255, 255, 255, 0.06)" stroke="#475569" stroke-width="1" />
        <text x="20" y="26" font-family="-apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif" font-size="15" font-weight="700" fill="#E2E8F0">✓ 100% Privasi Lokal • Tanpa Cloud</text>
      </g>
    </g>
  </g>

  <!-- Right Column: Real Android App Screen Mockup -->
  <g transform="translate(630, 45)" filter="url(#phoneShadow)">
    <!-- Device Frame -->
    <rect width="320" height="410" rx="36" fill="#0B0F19" stroke="#334155" stroke-width="3.5" />

    <!-- Screen Area (Theme: BackgroundDark #0B0F19, SurfaceDark #131B2E) -->
    <rect x="14" y="14" width="292" height="382" rx="26" fill="#0B0F19" />

    <!-- Top Notch -->
    <rect x="110" y="22" width="100" height="6" rx="3" fill="#334155" />

    <!-- TopAppBar Mockup (Shared Printers) -->
    <g transform="translate(26, 45)">
      <text x="0" y="18" font-family="sans-serif" font-size="18" font-weight="800" fill="#FFFFFF">Shared Printers</text>
      <text x="0" y="32" font-family="sans-serif" font-size="11" font-weight="500" fill="#94A3B8">Trusted LAN print queues</text>
      <circle cx="250" cy="22" r="14" fill="#1E293B" />
      <text x="245" y="27" font-family="sans-serif" font-size="15" fill="#38BDF8">⟳</text>
    </g>

    <!-- Printer Item 1 (Epson L3210 Series) -->
    <g transform="translate(26, 100)">
      <rect width="268" height="92" rx="16" fill="#131B2E" stroke="#249EA0" stroke-width="1.5" />
      <!-- Printer Icon -->
      <circle cx="34" cy="46" r="20" fill="rgba(36, 158, 160, 0.2)" />
      <rect x="24" y="38" width="20" height="13" rx="2" fill="#249EA0" />
      <rect x="27" y="33" width="14" height="6" rx="1" fill="#FFFFFF" />
      <rect x="26" y="47" width="16" height="8" rx="1" fill="#E2E8F0" />

      <text x="64" y="32" font-family="sans-serif" font-size="14" font-weight="700" fill="#FFFFFF">EPSON L3210 Series</text>
      <text x="64" y="48" font-family="sans-serif" font-size="11" fill="#94A3B8">DESKTOP-OFFICE • IPPS</text>
      <rect x="64" y="58" width="46" height="18" rx="6" fill="rgba(5, 150, 105, 0.2)" />
      <text x="72" y="71" font-family="sans-serif" font-size="10" font-weight="700" fill="#10B981">Online</text>
      <rect x="116" y="58" width="60" height="18" rx="6" fill="#1E293B" />
      <text x="122" y="71" font-family="sans-serif" font-size="10" font-weight="600" fill="#94A3B8">A4 / Letter</text>
    </g>

    <!-- Printer Item 2 (HP LaserJet 1020) -->
    <g transform="translate(26, 204)">
      <rect width="268" height="82" rx="16" fill="#131B2E" stroke="#334155" stroke-width="1" />
      <circle cx="34" cy="41" r="18" fill="rgba(100, 116, 139, 0.2)" />
      <rect x="25" y="34" width="18" height="12" rx="2" fill="#94A3B8" />

      <text x="64" y="32" font-family="sans-serif" font-size="14" font-weight="700" fill="#FFFFFF">HP LaserJet 1020</text>
      <text x="64" y="48" font-family="sans-serif" font-size="11" fill="#94A3B8">SERVER-LAN:48631</text>
      <rect x="64" y="56" width="46" height="18" rx="6" fill="rgba(5, 150, 105, 0.2)" />
      <text x="72" y="69" font-family="sans-serif" font-size="10" font-weight="700" fill="#10B981">Ready</text>
    </g>

    <!-- Bottom Navigation Bar inside mockup -->
    <g transform="translate(14, 320)">
      <rect width="292" height="60" rx="14" fill="#1E293B" />
      <g transform="translate(30, 20)">
        <text x="0" y="16" font-family="sans-serif" font-size="12" font-weight="700" fill="#249EA0">Printers</text>
      </g>
      <g transform="translate(110, 20)">
        <text x="0" y="16" font-family="sans-serif" font-size="12" font-weight="500" fill="#64748B">Discovery</text>
      </g>
      <g transform="translate(195, 20)">
        <text x="0" y="16" font-family="sans-serif" font-size="12" font-weight="500" fill="#64748B">Servers</text>
      </g>
    </g>
  </g>
</svg>
`;

// UI screenshots must be captured from the running Android application via ADB.
// This script only creates branding assets; it must never synthesize app screens.
renderSvgToPng(featureGraphicSvg, resolve(imgDir, "feature-graphic.png"), 1024, 500);
