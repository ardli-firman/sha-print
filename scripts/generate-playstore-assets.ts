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

// Helper for Phone Screenshots (1080x1920)
function createScreenshotSvg(props: {
  headline: string;
  subheadline: string;
  pillText: string;
  renderScreenContent: string;
  accentColor: string;
}) {
  return `
<svg width="1080" height="1920" viewBox="0 0 1080 1920" xmlns="http://www.w3.org/2000/svg">
  <defs>
    <linearGradient id="bgScreen" x1="0%" y1="0%" x2="0%" y2="100%">
      <stop offset="0%" stop-color="#0B132B" />
      <stop offset="35%" stop-color="#141F21" />
      <stop offset="100%" stop-color="#1B292B" />
    </linearGradient>
    <linearGradient id="phoneBody" x1="0%" y1="0%" x2="0%" y2="100%">
      <stop offset="0%" stop-color="#0B0F19" />
      <stop offset="100%" stop-color="#131B2E" />
    </linearGradient>
    <filter id="phoneShadow" x="-20%" y="-10%" width="140%" height="130%">
      <feDropShadow dx="0" dy="30" stdDeviation="40" flood-color="#000000" flood-opacity="0.65" />
    </filter>
  </defs>

  <!-- Background -->
  <rect width="1080" height="1920" fill="url(#bgScreen)" />

  <!-- Ambient Light Top in ShaPrint Teal/Cyan -->
  <circle cx="540" cy="200" r="450" fill="${props.accentColor}" opacity="0.14" filter="blur(90px)" />

  <!-- Header Section with Official App Logo Header -->
  <g transform="translate(90, 80)">
    <!-- Small Brand Anchor -->
    <g transform="translate(0, 0)">
      <g transform="scale(0.18)">
        <path fill="#249EA0" d="M 46 38 H 121 V 218 C 88.5 204 46 167 46 120 Z" />
        <path fill="#FFFFFF" fill-rule="evenodd" d="M 135 38 H 166 L 210 82 V 120 C 210 167 167.5 204 135 218 Z M 160 48 V 88 H 200 Z" />
      </g>
      <text x="45" y="27" font-family="-apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif" font-size="20" font-weight="900" fill="#FFFFFF">ShaPrint</text>
    </g>

    <!-- Category Pill -->
    <g transform="translate(0, 50)">
      <rect x="0" y="0" width="260" height="40" rx="20" fill="rgba(36,158,160,0.15)" stroke="${props.accentColor}" stroke-width="2" />
      <text x="130" y="26" font-family="-apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif" font-size="14" font-weight="800" fill="${props.accentColor}" text-anchor="middle" letter-spacing="1.5">${props.pillText}</text>
    </g>

    <!-- Main Headline -->
    <text x="0" y="150" font-family="-apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif" font-size="48" font-weight="900" fill="#FFFFFF">
      ${props.headline}
    </text>

    <!-- Subheadline -->
    <text x="0" y="195" font-family="-apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif" font-size="22" font-weight="500" fill="#94A3B8">
      ${props.subheadline}
    </text>
  </g>

  <!-- Phone Mockup Frame -->
  <g transform="translate(140, 360)" filter="url(#phoneShadow)">
    <!-- Outer Bezel -->
    <rect x="0" y="0" width="800" height="1520" rx="64" fill="#020617" stroke="#334155" stroke-width="6" />

    <!-- Screen Glass (Dark Surface #0B0F19) -->
    <rect x="20" y="20" width="760" height="1480" rx="48" fill="url(#phoneBody)" />

    <!-- Camera Island Notch -->
    <rect x="280" y="36" width="200" height="28" rx="14" fill="#020617" />
    <circle cx="430" cy="50" r="6" fill="#1E293B" />

    <!-- Screen Content Slot -->
    <g transform="translate(20, 80)">
      ${props.renderScreenContent}
    </g>
  </g>
</svg>
`;
}

// Screenshot 1: Discovery Screen (reflecting actual DiscoveryScreen.kt)
const screenshot1Svg = createScreenshotSvg({
  pillText: "ZERO-CONFIG mDNS",
  headline: "Temukan Server &amp; Printer",
  subheadline: "Deteksi server desktop lokal &amp; cross-VLAN tanpa setting manual",
  accentColor: "#249EA0",
  renderScreenContent: `
    <!-- TopAppBar from DiscoveryScreen.kt -->
    <g transform="translate(30, 20)">
      <text x="0" y="32" font-family="sans-serif" font-size="28" font-weight="800" fill="#FFFFFF">Network Discovery</text>
      <text x="0" y="56" font-family="sans-serif" font-size="15" font-weight="500" fill="#94A3B8">Locate ShaPrint desktop servers</text>
      <!-- Add IP button -->
      <rect x="560" y="12" width="140" height="44" rx="12" fill="#1E293B" stroke="#334155" stroke-width="1.5" />
      <text x="630" y="39" font-family="sans-serif" font-size="16" font-weight="700" fill="#2DD4BF" text-anchor="middle">+ Add IP</text>
    </g>

    <!-- Active Scanner Banner (Multi-VLAN mDNS Scanner Active) -->
    <g transform="translate(30, 110)">
      <rect width="700" height="96" rx="20" fill="rgba(36, 158, 160, 0.15)" stroke="#249EA0" stroke-width="2" />
      <circle cx="50" cy="48" r="22" fill="#249EA0" />
      <!-- Radar Waves -->
      <path d="M42 48 Q50 40 58 48" fill="none" stroke="#FFFFFF" stroke-width="3" stroke-linecap="round" />
      <circle cx="50" cy="54" r="3" fill="#FFFFFF" />

      <text x="90" y="42" font-family="sans-serif" font-size="20" font-weight="800" fill="#FFFFFF">Multi-VLAN mDNS Scanner Active</text>
      <text x="90" y="70" font-family="sans-serif" font-size="15" font-weight="500" fill="#A7F3D0">UDP 48633 • TTL=32 • Pinned SHA-256 TLS Fingerprint</text>
    </g>

    <!-- Scan Trigger Button with Ripple Animation -->
    <g transform="translate(30, 230)">
      <rect width="700" height="64" rx="18" fill="linear-gradient(90deg, #249EA0, #0284C7)" />
      <text x="350" y="40" font-family="sans-serif" font-size="20" font-weight="800" fill="#FFFFFF" text-anchor="middle">⟳ Scan Nearby Servers</text>
    </g>

    <!-- Server List Header -->
    <g transform="translate(30, 325)">
      <text x="0" y="20" font-family="sans-serif" font-size="17" font-weight="700" fill="#94A3B8" letter-spacing="1">NEARBY SERVERS (2 DISCOVERED)</text>
    </g>

    <!-- Server Card 1: Windows 11 Desktop -->
    <g transform="translate(30, 360)">
      <rect width="700" height="210" rx="24" fill="#131B2E" stroke="#249EA0" stroke-width="2" />
      <circle cx="60" cy="60" r="28" fill="rgba(36,158,160,0.2)" />
      <!-- Computer icon -->
      <rect x="44" y="48" width="32" height="22" rx="3" fill="#249EA0" />
      <rect x="56" y="70" width="8" height="6" fill="#249EA0" />
      <line x1="48" y1="76" x2="72" y2="76" stroke="#249EA0" stroke-width="3" stroke-linecap="round" />

      <text x="110" y="52" font-family="sans-serif" font-size="22" font-weight="800" fill="#FFFFFF">DESKTOP-OFFICE (Windows 11)</text>
      <text x="110" y="78" font-family="sans-serif" font-size="16" font-weight="500" fill="#38BDF8">192.168.1.105:48631 • IPPS Server</text>

      <line x1="30" y1="110" x2="670" y2="110" stroke="#1E293B" stroke-width="1.5" />

      <text x="30" y="145" font-family="sans-serif" font-size="16" font-weight="600" fill="#CBD5E1">Shared Printers:</text>
      <rect x="30" y="160" width="180" height="30" rx="8" fill="#1E293B" />
      <text x="40" y="181" font-family="sans-serif" font-size="14" font-weight="600" fill="#E2E8F0">🖨️ Epson L3210</text>
      <rect x="225" y="160" width="190" height="30" rx="8" fill="#1E293B" />
      <text x="235" y="181" font-family="sans-serif" font-size="14" font-weight="600" fill="#E2E8F0">🖨️ HP LaserJet 1020</text>

      <rect x="530" y="145" width="140" height="44" rx="12" fill="#059669" />
      <text x="600" y="172" font-family="sans-serif" font-size="15" font-weight="700" fill="#FFFFFF" text-anchor="middle">Review &amp; Trust</text>
    </g>

    <!-- Server Card 2: Linux Headless Server -->
    <g transform="translate(30, 600)">
      <rect width="700" height="175" rx="24" fill="#131B2E" stroke="#334155" stroke-width="1.5" />
      <circle cx="60" cy="60" r="28" fill="rgba(100,116,139,0.2)" />
      <rect x="44" y="48" width="32" height="22" rx="3" fill="#94A3B8" />

      <text x="110" y="52" font-family="sans-serif" font-size="22" font-weight="800" fill="#FFFFFF">LINUX-PRINT-DAEMON</text>
      <text x="110" y="78" font-family="sans-serif" font-size="16" font-weight="500" fill="#94A3B8">10.0.50.2:48631 • Cross-VLAN Reachable</text>

      <line x1="30" y1="110" x2="670" y2="110" stroke="#1E293B" stroke-width="1.5" />
      <text x="30" y="145" font-family="sans-serif" font-size="15" font-weight="600" fill="#64748B">Canon TS-300 Series (Online)</text>
      <rect x="550" y="125" width="120" height="40" rx="12" fill="#0284C7" />
      <text x="610" y="150" font-family="sans-serif" font-size="14" font-weight="700" fill="#FFFFFF" text-anchor="middle">Connect</text>
    </g>
  `,
});

// Screenshot 2: PrintersScreen (reflecting actual PrintersScreen.kt)
const screenshot2Svg = createScreenshotSvg({
  pillText: "TRUSTED QUEUES",
  headline: "Antrean Printer Siap Pakai",
  subheadline: "Kelola printer kantor &amp; rumah dalam satu aplikasi modern",
  accentColor: "#0284C7",
  renderScreenContent: `
    <!-- TopAppBar from PrintersScreen.kt -->
    <g transform="translate(30, 20)">
      <text x="0" y="32" font-family="sans-serif" font-size="28" font-weight="800" fill="#FFFFFF">Shared Printers</text>
      <text x="0" y="56" font-family="sans-serif" font-size="15" font-weight="500" fill="#94A3B8">Trusted LAN print queues</text>
      <circle cx="670" cy="30" r="18" fill="#1E293B" />
      <text x="663" y="36" font-family="sans-serif" font-size="18" fill="#38BDF8">⟳</text>
    </g>

    <!-- Printer 1: Epson L3210 Series -->
    <g transform="translate(30, 100)">
      <rect width="700" height="230" rx="24" fill="#131B2E" stroke="#249EA0" stroke-width="2" />
      <!-- Printer Avatar -->
      <circle cx="65" cy="65" r="32" fill="rgba(36,158,160,0.2)" />
      <rect x="50" y="54" width="30" height="20" rx="3" fill="#249EA0" />
      <rect x="54" y="46" width="22" height="10" rx="2" fill="#FFFFFF" />
      <rect x="53" y="68" width="24" height="12" rx="2" fill="#E2E8F0" />

      <text x="120" y="55" font-family="sans-serif" font-size="24" font-weight="800" fill="#FFFFFF">EPSON L3210 Series</text>
      <text x="120" y="82" font-family="sans-serif" font-size="16" fill="#94A3B8">DESKTOP-OFFICE • /ipp/print/epson_l3210</text>

      <!-- Status badge -->
      <g transform="translate(560, 42)">
        <rect width="110" height="34" rx="17" fill="rgba(5, 150, 105, 0.2)" />
        <circle cx="20" cy="17" r="5" fill="#10B981" />
        <text x="34" y="22" font-family="sans-serif" font-size="14" font-weight="700" fill="#10B981">Online</text>
      </g>

      <line x1="30" y1="120" x2="670" y2="120" stroke="#1E293B" stroke-width="1.5" />

      <!-- Capabilities Pills -->
      <g transform="translate(30, 140)">
        <rect x="0" y="0" width="125" height="32" rx="8" fill="#1E293B" />
        <text x="14" y="21" font-family="sans-serif" font-size="14" font-weight="600" fill="#CBD5E1">📄 ISO A4</text>

        <rect x="140" y="0" width="135" height="32" rx="8" fill="#1E293B" />
        <text x="154" y="21" font-family="sans-serif" font-size="14" font-weight="600" fill="#CBD5E1">📄 US Letter</text>

        <rect x="290" y="0" width="120" height="32" rx="8" fill="#1E293B" />
        <text x="304" y="21" font-family="sans-serif" font-size="14" font-weight="600" fill="#38BDF8">🎨 Full Color</text>

        <rect x="425" y="0" width="120" height="32" rx="8" fill="#1E293B" />
        <text x="439" y="21" font-family="sans-serif" font-size="14" font-weight="600" fill="#CBD5E1">Simplex</text>
      </g>

      <!-- Action Button -->
      <g transform="translate(30, 182)">
        <rect width="640" height="36" rx="10" fill="rgba(36,158,160,0.12)" />
        <text x="320" y="24" font-family="sans-serif" font-size="14" font-weight="700" fill="#2DD4BF" text-anchor="middle">Print Test Page (300 DPI PWG Raster)</text>
      </g>
    </g>

    <!-- Printer 2: Canon TS300 Series -->
    <g transform="translate(30, 360)">
      <rect width="700" height="170" rx="24" fill="#131B2E" stroke="#334155" stroke-width="1.5" />
      <circle cx="65" cy="65" r="32" fill="rgba(100,116,139,0.2)" />
      <rect x="50" y="54" width="30" height="20" rx="3" fill="#94A3B8" />

      <text x="120" y="55" font-family="sans-serif" font-size="24" font-weight="800" fill="#FFFFFF">Canon TS300 Series</text>
      <text x="120" y="82" font-family="sans-serif" font-size="16" fill="#94A3B8">LINUX-DAEMON • IPPS Shared</text>

      <g transform="translate(560, 42)">
        <rect width="110" height="34" rx="17" fill="rgba(5, 150, 105, 0.2)" />
        <circle cx="20" cy="17" r="5" fill="#10B981" />
        <text x="34" y="22" font-family="sans-serif" font-size="14" font-weight="700" fill="#10B981">Ready</text>
      </g>

      <line x1="30" y1="110" x2="670" y2="110" stroke="#1E293B" stroke-width="1.5" />
      <text x="30" y="142" font-family="sans-serif" font-size="15" fill="#94A3B8">A4, Photo 4×6" • Monochrome • Duplex: Long-Edge</text>
    </g>
  `,
});

// Screenshot 3: PrintPreviewScreen (reflecting PrintPreviewScreen.kt)
const screenshot3Svg = createScreenshotSvg({
  pillText: "PWG RASTER 300 DPI",
  headline: "Pratinjau &amp; Opsi Lengkap",
  subheadline: "Rendering dokumen akurat langsung di perangkat tanpa cloud",
  accentColor: "#F59E0B",
  renderScreenContent: `
    <!-- TopAppBar -->
    <g transform="translate(30, 20)">
      <text x="0" y="32" font-family="sans-serif" font-size="28" font-weight="800" fill="#FFFFFF">Print Job</text>
      <text x="0" y="56" font-family="sans-serif" font-size="15" font-weight="500" fill="#94A3B8">Document &amp; Print Settings</text>
      <rect x="580" y="14" width="120" height="40" rx="12" fill="#1E293B" />
      <text x="640" y="39" font-family="sans-serif" font-size="15" font-weight="700" fill="#F59E0B" text-anchor="middle">Share PDF</text>
    </g>

    <!-- Document Canvas with ShaPrint Watermark Header -->
    <g transform="translate(130, 95)">
      <rect width="500" height="660" rx="16" fill="#FFFFFF" stroke="#F59E0B" stroke-width="2.5" />
      
      <!-- Document Header Graphic -->
      <g transform="translate(40, 40)">
        <rect width="140" height="20" rx="4" fill="#0284C7" />
        <rect x="300" y="0" width="120" height="12" rx="3" fill="#94A3B8" />
        <line x1="0" y1="40" x2="420" y2="40" stroke="#E2E8F0" stroke-width="2" />
        <rect x="0" y="65" width="220" height="14" rx="3" fill="#0F172A" />
        <rect x="0" y="92" width="180" height="12" rx="3" fill="#64748B" />

        <!-- Mockup Invoice Table -->
        <rect x="0" y="130" width="420" height="34" rx="6" fill="#F1F5F9" />
        <rect x="0" y="175" width="420" height="34" rx="6" fill="#F8FAFC" />
        <rect x="0" y="220" width="420" height="34" rx="6" fill="#F1F5F9" />
        <rect x="0" y="265" width="420" height="34" rx="6" fill="#F8FAFC" />

        <!-- Chart Graphic in Document -->
        <rect x="0" y="330" width="420" height="150" rx="10" fill="#F8FAFC" />
        <rect x="40" y="420" width="40" height="40" rx="4" fill="#249EA0" />
        <rect x="110" y="380" width="40" height="80" rx="4" fill="#0284C7" />
        <rect x="180" y="350" width="40" height="110" rx="4" fill="#38BDF8" />
        <rect x="250" y="390" width="40" height="70" rx="4" fill="#6366F1" />
      </g>

      <!-- Badge 300 DPI -->
      <rect x="350" y="610" width="130" height="34" rx="8" fill="#1E293B" />
      <text x="415" y="632" font-family="sans-serif" font-size="13" font-weight="800" fill="#10B981" text-anchor="middle">300 DPI PWG</text>
    </g>

    <!-- Page Selector Controls -->
    <g transform="translate(260, 775)">
      <rect width="240" height="44" rx="22" fill="#1E293B" stroke="#334155" stroke-width="1.5" />
      <text x="35" y="28" font-family="sans-serif" font-size="18" fill="#94A3B8">◀</text>
      <text x="120" y="28" font-family="sans-serif" font-size="16" font-weight="700" fill="#FFFFFF" text-anchor="middle">Page 1 of 3</text>
      <text x="205" y="28" font-family="sans-serif" font-size="18" fill="#94A3B8">▶</text>
    </g>

    <!-- Settings Card -->
    <g transform="translate(30, 835)">
      <rect width="700" height="120" rx="20" fill="#131B2E" stroke="#334155" stroke-width="1.5" />
      <text x="30" y="40" font-family="sans-serif" font-size="16" fill="#94A3B8">Paper Size: <tspan fill="#FFFFFF" font-weight="700">ISO A4 (210×297mm)</tspan></text>
      <text x="380" y="40" font-family="sans-serif" font-size="16" fill="#94A3B8">Copies: <tspan fill="#FFFFFF" font-weight="700">1 Copy</tspan></text>
      <text x="30" y="85" font-family="sans-serif" font-size="16" fill="#94A3B8">Color Mode: <tspan fill="#10B981" font-weight="700">Full Color</tspan></text>
      <text x="380" y="85" font-family="sans-serif" font-size="16" fill="#94A3B8">Target: <tspan fill="#38BDF8" font-weight="700">EPSON L3210 Series</tspan></text>
    </g>

    <!-- Bottom Submit Button -->
    <g transform="translate(30, 975)">
      <rect width="700" height="66" rx="20" fill="linear-gradient(90deg, #0284C7, #249EA0)" />
      <text x="350" y="42" font-family="sans-serif" font-size="22" font-weight="800" fill="#FFFFFF" text-anchor="middle">Send Print Job to EPSON L3210</text>
    </g>
  `,
});

// Screenshot 4: ServersScreen & Keystore Security (reflecting ServersScreen.kt)
const screenshot4Svg = createScreenshotSvg({
  pillText: "HARDWARE KEYSTORE &amp; TLS",
  headline: "Keamanan &amp; Privasi Terjamin",
  subheadline: "Autentikasi Network Channel dengan sertifikat TLS terpinning",
  accentColor: "#2DD4BF",
  renderScreenContent: `
    <!-- TopAppBar from ServersScreen.kt -->
    <g transform="translate(30, 20)">
      <text x="0" y="32" font-family="sans-serif" font-size="28" font-weight="800" fill="#FFFFFF">Trusted Servers</text>
      <text x="0" y="56" font-family="sans-serif" font-size="15" font-weight="500" fill="#94A3B8">Managed server certificates &amp; keys</text>
    </g>

    <!-- Big Shield Visual -->
    <g transform="translate(290, 105)">
      <circle cx="90" cy="85" r="80" fill="rgba(36, 158, 160, 0.15)" stroke="#249EA0" stroke-width="3" />
      <!-- Logo inside shield -->
      <g transform="translate(68, 52) scale(0.24)">
        <path fill="#249EA0" d="M 46 38 H 121 V 218 C 88.5 204 46 167 46 120 Z" />
        <path fill="#FFFFFF" fill-rule="evenodd" d="M 135 38 H 166 L 210 82 V 120 C 210 167 167.5 204 135 218 Z M 160 48 V 88 H 200 Z" />
      </g>
    </g>

    <!-- Security Features from ShaPrint Architecture -->
    <g transform="translate(30, 295)">
      <!-- Item 1: Android Keystore Hardware -->
      <g transform="translate(0, 0)">
        <rect width="700" height="120" rx="20" fill="#131B2E" stroke="#334155" stroke-width="1.5" />
        <circle cx="55" cy="60" r="24" fill="rgba(16,185,129,0.2)" />
        <text x="46" y="68" font-family="sans-serif" font-size="24">🔑</text>
        <text x="95" y="48" font-family="sans-serif" font-size="20" font-weight="800" fill="#FFFFFF">Android Keystore Hardware TEE</text>
        <text x="95" y="78" font-family="sans-serif" font-size="15" fill="#94A3B8">Kredensial Network Channel dienkripsi di hardware security chip</text>
      </g>

      <!-- Item 2: SHA-256 TLS Fingerprint Pinning -->
      <g transform="translate(0, 140)">
        <rect width="700" height="150" rx="20" fill="#131B2E" stroke="#249EA0" stroke-width="2" />
        <circle cx="55" cy="75" r="24" fill="rgba(36,158,160,0.2)" />
        <text x="46" y="83" font-family="sans-serif" font-size="24">🔒</text>
        <text x="95" y="48" font-family="sans-serif" font-size="20" font-weight="800" fill="#FFFFFF">Pinned SHA-256 TLS Certificate</text>
        <text x="95" y="78" font-family="sans-serif" font-size="15" fill="#2DD4BF">Trust-on-First-Use: Mencegah serangan MITM antar subnet</text>
        
        <!-- Fingerprint display pill -->
        <rect x="95" y="98" width="560" height="32" rx="8" fill="#0B0F19" />
        <text x="110" y="120" font-family="monospace" font-size="14" font-weight="700" fill="#38BDF8">SHA256: E9A4 • 5B12 • 88C0 • 33D1 • 7F4A • 92B6</text>
      </g>

      <!-- Item 3: 100% Local LAN / Zero Cloud -->
      <g transform="translate(0, 310)">
        <rect width="700" height="120" rx="20" fill="#131B2E" stroke="#334155" stroke-width="1.5" />
        <circle cx="55" cy="60" r="24" fill="rgba(2,132,199,0.2)" />
        <text x="46" y="68" font-family="sans-serif" font-size="24">🛡️</text>
        <text x="95" y="48" font-family="sans-serif" font-size="20" font-weight="800" fill="#FFFFFF">100% Privasi Jaringan Lokal</text>
        <text x="95" y="78" font-family="sans-serif" font-size="15" fill="#94A3B8">Dokumen cetak tidak pernah dikirim ke internet atau server cloud</text>
      </g>
    </g>
  `,
});

console.log("Generating Play Store listing assets in img/...");

renderSvgToPng(featureGraphicSvg, resolve(imgDir, "feature-graphic.png"), 1024, 500);
renderSvgToPng(screenshot1Svg, resolve(imgDir, "screenshot-1-discovery.png"), 1080, 1920);
renderSvgToPng(screenshot2Svg, resolve(imgDir, "screenshot-2-system-print.png"), 1080, 1920);
renderSvgToPng(screenshot3Svg, resolve(imgDir, "screenshot-3-document-preview.png"), 1080, 1920);
renderSvgToPng(screenshot4Svg, resolve(imgDir, "screenshot-4-security-trust.png"), 1080, 1920);

console.log("All Play Store listing assets successfully generated with authentic project branding!");
