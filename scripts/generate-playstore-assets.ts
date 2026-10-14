import { Resvg } from "@resvg/resvg-js";
import { writeFileSync, mkdirSync } from "node:fs";
import { resolve } from "node:path";

const rootDir = resolve(import.meta.dirname, "..");
const imgDir = resolve(rootDir, "img");
mkdirSync(imgDir, { recursive: true });

function renderSvgToPng(svgStr: string, outPath: string, width?: number, height?: number) {
  const resvg = new Resvg(svgStr, {
    fitTo: width && height ? { mode: "width", value: width } : undefined,
  });
  const pngData = resvg.render();
  const pngBuffer = pngData.asPng();
  writeFileSync(outPath, pngBuffer);
  console.log(`Rendered: ${outPath} (${pngData.width}x${pngData.height})`);
}

// 1. App Icon (512x512)
const iconSvg = `
<svg width="512" height="512" viewBox="0 0 512 512" xmlns="http://www.w3.org/2000/svg">
  <defs>
    <linearGradient id="bgGrad" x1="0%" y1="0%" x2="100%" y2="100%">
      <stop offset="0%" stop-color="#1E3A8A" />
      <stop offset="50%" stop-color="#0284C7" />
      <stop offset="100%" stop-color="#0F172A" />
    </linearGradient>
    <linearGradient id="sheetGrad" x1="0%" y1="0%" x2="0%" y2="100%">
      <stop offset="0%" stop-color="#FFFFFF" />
      <stop offset="100%" stop-color="#E2E8F0" />
    </linearGradient>
    <linearGradient id="printerGrad" x1="0%" y1="0%" x2="0%" y2="100%">
      <stop offset="0%" stop-color="#0F172A" />
      <stop offset="100%" stop-color="#1E293B" />
    </linearGradient>
    <linearGradient id="glowGrad" x1="0%" y1="0%" x2="100%" y2="0%">
      <stop offset="0%" stop-color="#38BDF8" />
      <stop offset="100%" stop-color="#818CF8" />
    </linearGradient>
    <filter id="dropShadow" x="-20%" y="-20%" width="140%" height="140%">
      <feDropShadow dx="0" dy="12" stdDeviation="16" flood-color="#000000" flood-opacity="0.45" />
    </filter>
  </defs>

  <!-- Background Rounded Rect -->
  <rect width="512" height="512" rx="112" fill="url(#bgGrad)" />

  <!-- Subtle glow ring -->
  <rect x="16" y="16" width="480" height="480" rx="96" fill="none" stroke="rgba(255,255,255,0.15)" stroke-width="4" />

  <!-- Top paper sheet feeder -->
  <g filter="url(#dropShadow)">
    <rect x="156" y="80" width="200" height="150" rx="16" fill="url(#sheetGrad)" />
    <!-- Printed lines on paper -->
    <rect x="180" y="110" width="152" height="10" rx="5" fill="#94A3B8" />
    <rect x="180" y="132" width="110" height="10" rx="5" fill="#CBD5E1" />
    <rect x="180" y="154" width="130" height="10" rx="5" fill="#CBD5E1" />
  </g>

  <!-- Main Printer Body -->
  <g filter="url(#dropShadow)">
    <rect x="96" y="180" width="320" height="190" rx="28" fill="url(#printerGrad)" stroke="#334155" stroke-width="4" />
    
    <!-- Status LED bar / Accent glow -->
    <rect x="136" y="210" width="240" height="12" rx="6" fill="url(#glowGrad)" />

    <!-- Paper Output Tray Slot -->
    <rect x="126" y="270" width="260" height="60" rx="12" fill="#020617" />
  </g>

  <!-- Emitted Output Paper Sheet -->
  <g filter="url(#dropShadow)">
    <rect x="146" y="295" width="220" height="135" rx="14" fill="url(#sheetGrad)" stroke="#E2E8F0" stroke-width="2" />
    <!-- Graphic diagram / doc on output sheet -->
    <rect x="176" y="325" width="160" height="10" rx="5" fill="#0284C7" />
    <rect x="176" y="348" width="120" height="10" rx="5" fill="#64748B" />
    <rect x="176" y="370" width="90" height="10" rx="5" fill="#94A3B8" />
    <!-- Green Check Badge on sheet -->
    <circle cx="330" cy="385" r="18" fill="#10B981" />
    <path d="M322 385 L328 391 L338 379" fill="none" stroke="#FFFFFF" stroke-width="4" stroke-linecap="round" stroke-linejoin="round" />
  </g>
</svg>
`;

// 2. Feature Graphic (1024x500)
const featureGraphicSvg = `
<svg width="1024" height="500" viewBox="0 0 1024 500" xmlns="http://www.w3.org/2000/svg">
  <defs>
    <linearGradient id="bgBanner" x1="0%" y1="0%" x2="100%" y2="100%">
      <stop offset="0%" stop-color="#090D16" />
      <stop offset="40%" stop-color="#0F172A" />
      <stop offset="100%" stop-color="#1E293B" />
    </linearGradient>
    <linearGradient id="brandGrad" x1="0%" y1="0%" x2="100%" y2="0%">
      <stop offset="0%" stop-color="#38BDF8" />
      <stop offset="50%" stop-color="#60A5FA" />
      <stop offset="100%" stop-color="#A78BFA" />
    </linearGradient>
    <linearGradient id="cardGrad" x1="0%" y1="0%" x2="0%" y2="100%">
      <stop offset="0%" stop-color="#1E293B" />
      <stop offset="100%" stop-color="#0F172A" />
    </linearGradient>
    <filter id="cardShadow" x="-20%" y="-20%" width="140%" height="140%">
      <feDropShadow dx="0" dy="16" stdDeviation="20" flood-color="#000000" flood-opacity="0.6" />
    </filter>
  </defs>

  <!-- Background -->
  <rect width="1024" height="500" fill="url(#bgBanner)" />

  <!-- Geometric Grid Pattern Lines -->
  <g stroke="rgba(255,255,255,0.04)" stroke-width="1">
    <line x1="0" y1="100" x2="1024" y2="100" />
    <line x1="0" y1="200" x2="1024" y2="200" />
    <line x1="0" y1="300" x2="1024" y2="300" />
    <line x1="0" y1="400" x2="1024" y2="400" />
    <line x1="200" y1="0" x2="200" y2="500" />
    <line x1="400" y1="0" x2="400" y2="500" />
    <line x1="600" y1="0" x2="600" y2="500" />
    <line x1="800" y1="0" x2="800" y2="500" />
  </g>

  <!-- Ambient Glow Blobs -->
  <circle cx="200" cy="150" r="220" fill="#0284C7" opacity="0.18" filter="blur(60px)" />
  <circle cx="850" cy="350" r="260" fill="#6366F1" opacity="0.15" filter="blur(80px)" />

  <!-- Left Column: Branding & Value Proposition -->
  <g transform="translate(80, 110)">
    <!-- Pill Badge -->
    <rect x="0" y="0" width="220" height="34" rx="17" fill="rgba(2, 132, 199, 0.2)" stroke="#38BDF8" stroke-width="1.5" />
    <text x="16" y="22" font-family="-apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif" font-size="13" font-weight="700" fill="#38BDF8" letter-spacing="1">STANDARDS-BASED IPPS</text>

    <!-- Main Title -->
    <text x="0" y="85" font-family="-apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif" font-size="52" font-weight="900" fill="#FFFFFF">ShaPrint</text>
    <text x="220" y="85" font-family="-apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif" font-size="52" font-weight="900" fill="url(#brandGrad)">Mobile</text>

    <!-- Subtitle / Tagline -->
    <text x="0" y="130" font-family="-apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif" font-size="19" font-weight="600" fill="#E2E8F0">
      LAN &amp; Cross-VLAN Printer Sharing for Android
    </text>

    <!-- Key Feature Pills -->
    <g transform="translate(0, 175)">
      <!-- Feature 1 -->
      <g transform="translate(0, 0)">
        <circle cx="12" cy="12" r="10" fill="#10B981" />
        <path d="M8 12 L11 15 L16 9" fill="none" stroke="#FFFFFF" stroke-width="2.5" stroke-linecap="round" />
        <text x="32" y="17" font-family="-apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif" font-size="15" font-weight="600" fill="#CBD5E1">Zero-Config mDNS Discovery</text>
      </g>
      <!-- Feature 2 -->
      <g transform="translate(0, 40)">
        <circle cx="12" cy="12" r="10" fill="#10B981" />
        <path d="M8 12 L11 15 L16 9" fill="none" stroke="#FFFFFF" stroke-width="2.5" stroke-linecap="round" />
        <text x="32" y="17" font-family="-apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif" font-size="15" font-weight="600" fill="#CBD5E1">Native Android PrintService Integration</text>
      </g>
      <!-- Feature 3 -->
      <g transform="translate(0, 80)">
        <circle cx="12" cy="12" r="10" fill="#10B981" />
        <path d="M8 12 L11 15 L16 9" fill="none" stroke="#FFFFFF" stroke-width="2.5" stroke-linecap="round" />
        <text x="32" y="17" font-family="-apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif" font-size="15" font-weight="600" fill="#CBD5E1">Hardware-Backed Keystore Security</text>
      </g>
    </g>
  </g>

  <!-- Right Column: Stylized App Mockup Cards -->
  <g transform="translate(630, 60)" filter="url(#cardShadow)">
    <!-- Main Device Card -->
    <rect x="0" y="0" width="310" height="380" rx="32" fill="url(#cardGrad)" stroke="#334155" stroke-width="3" />

    <!-- Card Header / Status Bar -->
    <rect x="110" y="14" width="90" height="6" rx="3" fill="#334155" />
    <circle cx="280" cy="17" r="4" fill="#10B981" />

    <!-- App Bar Inside Mockup -->
    <text x="24" y="60" font-family="-apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif" font-size="18" font-weight="800" fill="#FFFFFF">Shared Printers</text>
    <text x="24" y="78" font-family="-apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif" font-size="11" font-weight="500" fill="#94A3B8">LAN IPPS • Port 48631</text>

    <!-- Printer Item 1 -->
    <g transform="translate(20, 96)">
      <rect width="270" height="74" rx="16" fill="#1E293B" stroke="#0284C7" stroke-width="1.5" />
      <rect x="14" y="14" width="44" height="46" rx="10" fill="rgba(2,132,199,0.2)" />
      <!-- Printer Icon -->
      <rect x="22" y="24" width="28" height="18" rx="3" fill="#38BDF8" />
      <rect x="26" y="18" width="20" height="8" rx="2" fill="#E2E8F0" />
      <rect x="25" y="36" width="22" height="12" rx="2" fill="#FFFFFF" />
      
      <text x="68" y="32" font-family="-apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif" font-size="14" font-weight="700" fill="#FFFFFF">Epson L3210 Series</text>
      <text x="68" y="50" font-family="-apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif" font-size="11" font-weight="500" fill="#38BDF8">Ready • A4 / Letter</text>
      <!-- Ready Badge -->
      <circle cx="248" cy="37" r="6" fill="#10B981" />
    </g>

    <!-- Printer Item 2 -->
    <g transform="translate(20, 182)">
      <rect width="270" height="74" rx="16" fill="#0F172A" stroke="#334155" stroke-width="1" />
      <rect x="14" y="14" width="44" height="46" rx="10" fill="rgba(100,116,139,0.2)" />
      <!-- Printer Icon -->
      <rect x="22" y="24" width="28" height="18" rx="3" fill="#94A3B8" />
      <rect x="26" y="18" width="20" height="8" rx="2" fill="#CBD5E1" />
      <rect x="25" y="36" width="22" height="12" rx="2" fill="#E2E8F0" />

      <text x="68" y="32" font-family="-apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif" font-size="14" font-weight="700" fill="#E2E8F0">Canon TS300 Series</text>
      <text x="68" y="50" font-family="-apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif" font-size="11" font-weight="500" fill="#64748B">Office-PC • IPPS</text>
      <circle cx="248" cy="37" r="6" fill="#10B981" />
    </g>

    <!-- Action Button Mockup -->
    <g transform="translate(20, 274)">
      <rect width="270" height="48" rx="14" fill="url(#brandGrad)" />
      <text x="135" y="30" font-family="-apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif" font-size="14" font-weight="700" fill="#FFFFFF" text-anchor="middle">Scan Nearby Servers</text>
    </g>

    <!-- Floating Security Badge on side -->
    <g transform="translate(-30, 220)" filter="url(#cardShadow)">
      <rect width="130" height="44" rx="12" fill="#0F172A" stroke="#10B981" stroke-width="2" />
      <circle cx="22" cy="22" r="8" fill="#10B981" />
      <path d="M18 22 L21 25 L26 19" fill="none" stroke="#FFFFFF" stroke-width="2" stroke-linecap="round" />
      <text x="36" y="20" font-family="-apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif" font-size="11" font-weight="700" fill="#FFFFFF">TLS Verified</text>
      <text x="36" y="32" font-family="-apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif" font-size="9" font-weight="600" fill="#10B981">Zero Cloud</text>
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
      <stop offset="35%" stop-color="#0F172A" />
      <stop offset="100%" stop-color="#1E293B" />
    </linearGradient>
    <linearGradient id="phoneBody" x1="0%" y1="0%" x2="0%" y2="100%">
      <stop offset="0%" stop-color="#1E293B" />
      <stop offset="100%" stop-color="#0F172A" />
    </linearGradient>
    <filter id="phoneShadow" x="-20%" y="-10%" width="140%" height="130%">
      <feDropShadow dx="0" dy="30" stdDeviation="40" flood-color="#000000" flood-opacity="0.65" />
    </filter>
  </defs>

  <!-- Background -->
  <rect width="1080" height="1920" fill="url(#bgScreen)" />

  <!-- Ambient Light Top -->
  <circle cx="540" cy="200" r="450" fill="${props.accentColor}" opacity="0.12" filter="blur(90px)" />

  <!-- Header Section -->
  <g transform="translate(90, 110)">
    <!-- Category Pill -->
    <rect x="0" y="0" width="220" height="42" rx="21" fill="rgba(255,255,255,0.08)" stroke="${props.accentColor}" stroke-width="2" />
    <text x="110" y="27" font-family="-apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif" font-size="15" font-weight="800" fill="${props.accentColor}" text-anchor="middle" letter-spacing="1.5">${props.pillText}</text>

    <!-- Main Headline -->
    <text x="0" y="110" font-family="-apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif" font-size="52" font-weight="900" fill="#FFFFFF">
      ${props.headline}
    </text>

    <!-- Subheadline -->
    <text x="0" y="165" font-family="-apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif" font-size="24" font-weight="500" fill="#94A3B8">
      ${props.subheadline}
    </text>
  </g>

  <!-- Phone Mockup Frame -->
  <g transform="translate(140, 360)" filter="url(#phoneShadow)">
    <!-- Outer Bezel -->
    <rect x="0" y="0" width="800" height="1520" rx="64" fill="#020617" stroke="#334155" stroke-width="6" />

    <!-- Screen Glass -->
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

// Screenshot 1: Discovery Screen
const screenshot1Svg = createScreenshotSvg({
  pillText: "ZERO-CONFIG mDNS",
  headline: "Temukan Printer Otomatis",
  subheadline: "Deteksi printer lokal dan cross-VLAN tanpa konfigurasi rumit",
  accentColor: "#38BDF8",
  renderScreenContent: `
    <!-- App Bar -->
    <rect x="30" y="20" width="700" height="70" rx="16" fill="rgba(255,255,255,0.03)" />
    <text x="50" y="65" font-family="sans-serif" font-size="28" font-weight="800" fill="#FFFFFF">Nearby Servers</text>
    <rect x="620" y="35" width="90" height="40" rx="12" fill="#0284C7" />
    <text x="665" y="60" font-family="sans-serif" font-size="16" font-weight="700" fill="#FFFFFF" text-anchor="middle">Scan</text>

    <!-- Status Banner -->
    <g transform="translate(30, 110)">
      <rect width="700" height="90" rx="20" fill="rgba(16, 185, 129, 0.12)" stroke="#10B981" stroke-width="2" />
      <circle cx="45" cy="45" r="18" fill="#10B981" />
      <path d="M37 45 L43 51 L53 39" fill="none" stroke="#FFFFFF" stroke-width="3" stroke-linecap="round" />
      <text x="80" y="40" font-family="sans-serif" font-size="20" font-weight="700" fill="#FFFFFF">mDNS Discovery Aktif</text>
      <text x="80" y="66" font-family="sans-serif" font-size="16" font-weight="500" fill="#A7F3D0">Port UDP 48633 • TTL=32 (Cross-VLAN Ready)</text>
    </g>

    <!-- Server Card 1 -->
    <g transform="translate(30, 230)">
      <rect width="700" height="200" rx="24" fill="#1E293B" stroke="#38BDF8" stroke-width="2" />
      <circle cx="60" cy="60" r="28" fill="rgba(56,189,248,0.2)" />
      <!-- PC Icon -->
      <rect x="44" y="48" width="32" height="22" rx="3" fill="#38BDF8" />
      <rect x="56" y="70" width="8" height="6" fill="#38BDF8" />
      <line x1="48" y1="76" x2="72" y2="76" stroke="#38BDF8" stroke-width="3" stroke-linecap="round" />

      <text x="110" y="52" font-family="sans-serif" font-size="22" font-weight="800" fill="#FFFFFF">DESKTOP-OFFICE (Windows 11)</text>
      <text x="110" y="78" font-family="sans-serif" font-size="16" font-weight="500" fill="#38BDF8">192.168.1.105:48631 • IPPS Server</text>

      <line x1="30" y1="110" x2="670" y2="110" stroke="#334155" stroke-width="1.5" />

      <text x="30" y="145" font-family="sans-serif" font-size="17" font-weight="600" fill="#E2E8F0">Shared Printers (2):</text>
      <rect x="30" y="160" width="180" height="28" rx="8" fill="#0F172A" />
      <text x="40" y="179" font-family="sans-serif" font-size="14" font-weight="600" fill="#94A3B8">🖨️ Epson L3210</text>
      <rect x="225" y="160" width="180" height="28" rx="8" fill="#0F172A" />
      <text x="235" y="179" font-family="sans-serif" font-size="14" font-weight="600" fill="#94A3B8">🖨️ HP LaserJet 1020</text>

      <rect x="560" y="145" width="110" height="42" rx="12" fill="#10B981" />
      <text x="615" y="171" font-family="sans-serif" font-size="15" font-weight="700" fill="#FFFFFF" text-anchor="middle">Trust Server</text>
    </g>

    <!-- Server Card 2 -->
    <g transform="translate(30, 460)">
      <rect width="700" height="170" rx="24" fill="#1E293B" stroke="#334155" stroke-width="1.5" />
      <circle cx="60" cy="60" r="28" fill="rgba(100,116,139,0.2)" />
      <rect x="44" y="48" width="32" height="22" rx="3" fill="#94A3B8" />

      <text x="110" y="52" font-family="sans-serif" font-size="22" font-weight="800" fill="#FFFFFF">UBUNTU-SERVER (Linux Daemon)</text>
      <text x="110" y="78" font-family="sans-serif" font-size="16" font-weight="500" fill="#94A3B8">10.0.50.2:48631 • Cross-VLAN Reachable</text>

      <line x1="30" y1="110" x2="670" y2="110" stroke="#334155" stroke-width="1.5" />
      <text x="30" y="142" font-family="sans-serif" font-size="15" font-weight="600" fill="#64748B">Canon TS-300 Series (Online)</text>
      <rect x="560" y="118" width="110" height="42" rx="12" fill="#0284C7" />
      <text x="615" y="144" font-family="sans-serif" font-size="15" font-weight="700" fill="#FFFFFF" text-anchor="middle">Review</text>
    </g>
  `,
});

// Screenshot 2: Android Native PrintService
const screenshot2Svg = createScreenshotSvg({
  pillText: "SYSTEM PRINT SERVICE",
  headline: "Cetak Langsung dari Aplikasi",
  subheadline: "Terintegrasi penuh dengan Android Print Spooler &amp; dialog sistem",
  accentColor: "#10B981",
  renderScreenContent: `
    <!-- Android Native Print Spooler Dialog Mockup -->
    <rect x="30" y="20" width="700" height="800" rx="28" fill="#1E293B" stroke="#334155" stroke-width="2" />

    <!-- Print Spooler Header -->
    <text x="60" y="75" font-family="sans-serif" font-size="26" font-weight="800" fill="#FFFFFF">Print Preview</text>
    <rect x="590" y="45" width="110" height="46" rx="23" fill="#10B981" />
    <text x="645" y="74" font-family="sans-serif" font-size="18" font-weight="700" fill="#FFFFFF" text-anchor="middle">PRINT</text>

    <!-- Printer Selector Dropdown -->
    <g transform="translate(60, 110)">
      <rect width="640" height="85" rx="16" fill="#0F172A" stroke="#10B981" stroke-width="2" />
      <text x="24" y="34" font-family="sans-serif" font-size="14" font-weight="600" fill="#94A3B8">Selected Printer</text>
      <text x="24" y="62" font-family="sans-serif" font-size="20" font-weight="800" fill="#FFFFFF">Epson L3210 (via ShaPrint)</text>
      <!-- Down arrow -->
      <path d="M590 40 L600 52 L610 40" fill="none" stroke="#38BDF8" stroke-width="3" stroke-linecap="round" />
    </g>

    <!-- Document Thumbnail Mockup -->
    <g transform="translate(190, 225)">
      <rect width="380" height="520" rx="12" fill="#FFFFFF" />
      <!-- Invoice layout bars -->
      <rect x="30" y="30" width="120" height="24" rx="4" fill="#0284C7" />
      <rect x="230" y="30" width="120" height="14" rx="3" fill="#94A3B8" />
      <line x1="30" y1="80" x2="350" y2="80" stroke="#CBD5E1" stroke-width="2" />
      <rect x="30" y="110" width="200" height="12" rx="3" fill="#64748B" />
      <rect x="30" y="135" width="160" height="12" rx="3" fill="#94A3B8" />
      <!-- Table rows -->
      <rect x="30" y="180" width="320" height="40" rx="6" fill="#F1F5F9" />
      <rect x="30" y="235" width="320" height="40" rx="6" fill="#F8FAFC" />
      <rect x="30" y="290" width="320" height="40" rx="6" fill="#F1F5F9" />
      <rect x="200" y="360" width="150" height="30" rx="6" fill="#0284C7" />
    </g>

    <!-- Print Options Row -->
    <g transform="translate(60, 850)">
      <rect width="640" height="220" rx="20" fill="#0F172A" stroke="#334155" stroke-width="1" />
      <text x="30" y="45" font-family="sans-serif" font-size="18" font-weight="700" fill="#FFFFFF">Print Settings</text>
      
      <text x="30" y="90" font-family="sans-serif" font-size="15" fill="#94A3B8">Copies: <tspan fill="#FFFFFF" font-weight="700">1</tspan></text>
      <text x="240" y="90" font-family="sans-serif" font-size="15" fill="#94A3B8">Paper: <tspan fill="#FFFFFF" font-weight="700">ISO A4 (210×297 mm)</tspan></text>
      
      <text x="30" y="140" font-family="sans-serif" font-size="15" fill="#94A3B8">Color Mode: <tspan fill="#10B981" font-weight="700">Full Color</tspan></text>
      <text x="240" y="140" font-family="sans-serif" font-size="15" fill="#94A3B8">Quality: <tspan fill="#FFFFFF" font-weight="700">300 DPI PWG Raster</tspan></text>
    </g>
  `,
});

// Screenshot 3: High Fidelity Preview & Rasterizer
const screenshot3Svg = createScreenshotSvg({
  pillText: "PRECISE RENDERING",
  headline: "Pratinjau Presisi 300 DPI",
  subheadline: "Rendering PWG Raster langsung di perangkat tanpa cloud server",
  accentColor: "#F59E0B",
  renderScreenContent: `
    <!-- ShaPrint Print Screen -->
    <rect x="30" y="20" width="700" height="70" rx="16" fill="rgba(255,255,255,0.03)" />
    <text x="50" y="65" font-family="sans-serif" font-size="28" font-weight="800" fill="#FFFFFF">Document Preview</text>
    <rect x="600" y="35" width="110" height="40" rx="12" fill="#F59E0B" />
    <text x="655" y="60" font-family="sans-serif" font-size="16" font-weight="700" fill="#FFFFFF" text-anchor="middle">Share Doc</text>

    <!-- Document Canvas Preview with Page Indicator -->
    <g transform="translate(130, 120)">
      <rect width="500" height="680" rx="18" fill="#FFFFFF" stroke="#F59E0B" stroke-width="3" />
      <!-- Drawing content -->
      <circle cx="250" cy="180" r="70" fill="#FDE68A" />
      <polygon points="250,130 290,210 210,210" fill="#F59E0B" />
      <rect x="60" y="290" width="380" height="16" rx="4" fill="#0F172A" />
      <rect x="60" y="325" width="320" height="12" rx="4" fill="#64748B" />
      <rect x="60" y="350" width="350" height="12" rx="4" fill="#94A3B8" />
      <rect x="60" y="375" width="280" height="12" rx="4" fill="#CBD5E1" />

      <!-- High DPI Badge on corner -->
      <g transform="translate(360, 20)">
        <rect width="115" height="34" rx="8" fill="#0F172A" />
        <text x="57" y="22" font-family="sans-serif" font-size="13" font-weight="800" fill="#10B981" text-anchor="middle">300 DPI PWG</text>
      </g>
    </g>

    <!-- Page Selector Controls -->
    <g transform="translate(250, 830)">
      <rect width="260" height="48" rx="24" fill="#1E293B" stroke="#334155" stroke-width="1.5" />
      <text x="50" y="30" font-family="sans-serif" font-size="18" font-weight="700" fill="#94A3B8">◀</text>
      <text x="130" y="30" font-family="sans-serif" font-size="16" font-weight="700" fill="#FFFFFF" text-anchor="middle">Page 1 of 4</text>
      <text x="210" y="30" font-family="sans-serif" font-size="18" font-weight="700" fill="#94A3B8">▶</text>
    </g>

    <!-- Send to Printer Action Button -->
    <g transform="translate(50, 920)">
      <rect width="660" height="70" rx="20" fill="linear-gradient(90deg, #F59E0B, #EA580C)" />
      <text x="330" y="44" font-family="sans-serif" font-size="22" font-weight="800" fill="#FFFFFF" text-anchor="middle">Submit Print Job (IPPS)</text>
    </g>
  `,
});

// Screenshot 4: Security & Hardware Keystore
const screenshot4Svg = createScreenshotSvg({
  pillText: "SECURITY &amp; PRIVACY",
  headline: "Keamanan TLS &amp; Keystore",
  subheadline: "Autentikasi Network Channel dengan sertifikat TLS terverifikasi",
  accentColor: "#8B5CF6",
  renderScreenContent: `
    <!-- Security Overview Header -->
    <rect x="30" y="20" width="700" height="70" rx="16" fill="rgba(255,255,255,0.03)" />
    <text x="50" y="65" font-family="sans-serif" font-size="28" font-weight="800" fill="#FFFFFF">Security &amp; Trust</text>

    <!-- Big Shield Graphic -->
    <g transform="translate(290, 110)">
      <circle cx="90" cy="90" r="85" fill="rgba(139, 92, 246, 0.15)" stroke="#8B5CF6" stroke-width="3" />
      <path d="M90 35 L135 60 L135 110 Q90 155 90 155 Q45 110 45 60 Z" fill="#8B5CF6" />
      <path d="M75 90 L85 100 L108 76" fill="none" stroke="#FFFFFF" stroke-width="6" stroke-linecap="round" stroke-linejoin="round" />
    </g>

    <!-- Security Attributes List -->
    <g transform="translate(30, 310)">
      <!-- Item 1: Hardware Keystore -->
      <g transform="translate(0, 0)">
        <rect width="700" height="110" rx="20" fill="#1E293B" stroke="#334155" stroke-width="1.5" />
        <circle cx="55" cy="55" r="22" fill="rgba(16,185,129,0.2)" />
        <text x="47" y="62" font-family="sans-serif" font-size="20">🔑</text>
        <text x="95" y="45" font-family="sans-serif" font-size="19" font-weight="800" fill="#FFFFFF">Android Keystore Hardware Crypto</text>
        <text x="95" y="75" font-family="sans-serif" font-size="15" fill="#94A3B8">Kredensial Network Channel dienkripsi di hardware TEE</text>
      </g>

      <!-- Item 2: TLS Fingerprint Pinning -->
      <g transform="translate(0, 130)">
        <rect width="700" height="130" rx="20" fill="#1E293B" stroke="#8B5CF6" stroke-width="2" />
        <circle cx="55" cy="65" r="22" fill="rgba(139,92,246,0.2)" />
        <text x="47" y="72" font-family="sans-serif" font-size="20">🔒</text>
        <text x="95" y="45" font-family="sans-serif" font-size="19" font-weight="800" fill="#FFFFFF">SHA-256 TLS Fingerprint Pinning</text>
        <text x="95" y="75" font-family="sans-serif" font-size="15" fill="#A78BFA">Trust-on-First-Use: Mencegah serangan Man-in-the-Middle</text>
        <rect x="95" y="90" width="340" height="26" rx="6" fill="#0F172A" />
        <text x="105" y="108" font-family="monospace" font-size="12" font-weight="700" fill="#38BDF8">E9A4 • 5B12 • 88C0 • 33D1</text>
      </g>

      <!-- Item 3: 100% Local LAN Privacy -->
      <g transform="translate(0, 280)">
        <rect width="700" height="110" rx="20" fill="#1E293B" stroke="#334155" stroke-width="1.5" />
        <circle cx="55" cy="55" r="22" fill="rgba(2,132,199,0.2)" />
        <text x="47" y="62" font-family="sans-serif" font-size="20">🛡️</text>
        <text x="95" y="45" font-family="sans-serif" font-size="19" font-weight="800" fill="#FFFFFF">100% Privasi Jaringan Lokal</text>
        <text x="95" y="75" font-family="sans-serif" font-size="15" fill="#94A3B8">Tidak ada dokumen atau data yang dikirim ke cloud server</text>
      </g>
    </g>
  `,
});

console.log("Generating Play Store listing assets in img/...");

renderSvgToPng(iconSvg, resolve(imgDir, "app-icon.png"), 512, 512);
renderSvgToPng(featureGraphicSvg, resolve(imgDir, "feature-graphic.png"), 1024, 500);
renderSvgToPng(screenshot1Svg, resolve(imgDir, "screenshot-1-discovery.png"), 1080, 1920);
renderSvgToPng(screenshot2Svg, resolve(imgDir, "screenshot-2-system-print.png"), 1080, 1920);
renderSvgToPng(screenshot3Svg, resolve(imgDir, "screenshot-3-document-preview.png"), 1080, 1920);
renderSvgToPng(screenshot4Svg, resolve(imgDir, "screenshot-4-security-trust.png"), 1080, 1920);

console.log("All Play Store listing assets generated successfully!");
