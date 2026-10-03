# ShaPrint — Brand & Identity Guidelines

> **Version**: 1.0.0  
> **Status**: Approved  
> **Core Concept**: Two-Tone Peer Shield (Secure Document)

---

## 1. Brand Story & Concept

**ShaPrint** connects physical printers across local networks (LAN and cross-VLAN) using encrypted IPPS without reliance on third-party cloud servers.

The identity embodies three foundational principles:
1. **Security & Privacy (The Shield)**: The protective outer silhouette communicates end-to-end TLS security (port 8631), certificate fingerprint verification, and zero cloud data leaks.
2. **Printer & Paper (The Folded Dog-Ear)**: The 45-degree corner fold on the right wing identifies the mark indisputably as a print document and paper medium.
3. **Peer-to-Peer Sharing (The Central Seam)**: The symmetrical vertical split divides the mark into two collaborating peers: the **Client Workstation** (Spruce Teal) and the **Printer Server Host** (Dark Slate), united across a secure local network channel.

```
       Client Pillar              Server & Document
     (Workstation / IPPS)       (Print Host / Paper Fold)
             ┌─────────┐         ┌───────┐
             │         │         │      /│  <-- 45° Dog-Ear Paper Fold
             │         │         │     / │
             │         │         │    └──┤
             │         │  14px   │       │
             │         │ Channel │       │
             │         │  (LAN)  │       │
             \         /         \       /
              \       /           \     /
               \_____/             \___/
```

---

## 2. Color Palette & Design Tokens

The brand palette is derived directly from the desktop application's production theme:

| Role | Color Name | HEX | RGB | HSL | Usage |
|---|---|---|---|---|---|
| **Primary** | Spruce Teal | `#126b73` | `rgb(18, 107, 115)` | `184°, 73%, 26%` | Left client pillar, primary buttons, accents, "Print" text |
| **Secondary** | Dark Slate | `#1b292b` | `rgb(27, 41, 43)` | `188°, 23%, 14%` | Right server pillar, app icon background, primary typography |
| **Accent** | Vibrant Cyan | `#249ea0` | `rgb(36, 158, 160)` | `181°, 63%, 38%` | Network pulses, status badges, reversed text highlights |
| **Paper Tint** | Cool Mint | `#deebe8` | `rgb(222, 235, 232)` | `168°, 28%, 90%` | Badges, card backgrounds, hover states |
| **Canvas** | Crisp White | `#fcfdfb` | `rgb(252, 253, 251)` | `80°, 20%, 99%` | Light background canvas, paper highlights |

### CSS Variables (`apps/desktop/src/App.css`)
```css
:root {
  --color-brand-teal: #126b73;
  --color-brand-slate: #1b292b;
  --color-brand-accent: #249ea0;
  --color-brand-mint: #deebe8;
  --color-brand-paper: #fcfdfb;
}
```

---

## 3. Typography & Lockups

The wordmark pairs the custom two-tone symbol with **Instrument Sans** (with fallback to Segoe UI / system sans):

* **"Sha"**: Dark Slate `#1b292b`, Bold (700 weight), letter-spacing `-0.02em`
* **"Print"**: Spruce Teal `#126b73`, Bold (700 weight), letter-spacing `-0.02em`

### Available Lockups

1. **Horizontal Lockup** (`assets/branding/dist/shaprint-lockup-horizontal.svg`):
   * Primary lockup for application headers, navigation bars, and website footers.
   * Proportions: `700 × 256` (aspect ratio 2.73:1).
   * Fully outlined vector paths with zero live text dependencies.
2. **Stacked Lockup** (`assets/branding/dist/shaprint-lockup-stacked.svg`):
   * For splash screens, promotional banners, installers, and square avatars.
   * Proportions: `420 × 420` (aspect ratio 1:1) with generous optical clear space.
3. **Standalone Symbol** (`assets/branding/dist/shaprint-symbol.svg`):
   * Master symbol for system trays, app icons, favicons, and compact UI.
   * Proportions: `256 × 256` (aspect ratio 1:1).

---

## 4. Clear Space & Minimum Sizes

### Clear Space
Maintain clear space equal to the width of the dog-ear cutout ($X \approx \text{width} / 4$) around the symbol on all sides. No typography, borders, or graphics may enter this perimeter.

```
       +------------------------------------+
       |              X                     |
       |     +------------------+           |
       |  X  |      SYMBOL      |  X        |
       |     +------------------+           |
       |              X                     |
       +------------------------------------+
```

### Minimum Reproduction Sizes

* **Digital (Symbol)**: Minimum `16 × 16 px` (Taskbar / System Tray / Browser Tab).
* **Digital (Lockup)**: Minimum `24 px` height (`96 × 24 px`).
* **Print (Symbol)**: Minimum `8 mm × 8 mm`.
* **Print (Lockup)**: Minimum `12 mm` height.

---

## 5. App & Platform Icons (Tauri Desktop App)

The application icon uses the symbol placed on a dark slate rounded squircle tile:

* `apps/desktop/src-tauri/icons/icon.ico`: Multi-resolution Windows executable icon (`16, 24, 32, 48, 64, 128, 256 px`).
* `apps/desktop/src-tauri/icons/icon.png`: Master `512 × 512 px` app icon.
* `apps/desktop/src-tauri/icons/icon.icns`: Multi-resolution macOS bundle icon.
* `apps/desktop/src-tauri/icons/Square*.png`: Windows Start Menu / Taskbar tile logos.
* `apps/desktop/public/favicon.ico`: Multi-resolution web favicon (`16, 32, 48 px`).
* `apps/desktop/public/apple-touch-icon.png`: iOS / Safari web app icon (`180 × 180 px`).

---

## 6. Incorrect Usage (Do's and Don'ts)

* **DO** use the two-tone symbol on light backgrounds (`#ffffff`, `#fcfdfb`, `#eff3f1`).
* **DO** use the reversed white lockup (`shaprint-lockup-horizontal-white.svg`) on dark slate or black backgrounds.
* **DO** maintain the exact 14px optical channel between the two shield pillars.
* **DON'T** add drop shadows, outer glows, or 3D skeuomorphic gradients.
* **DON'T** stretch, skew, or rotate the mark away from vertical alignment.
* **DON'T** close the central network channel or fill it with solid color.
* **DON'T** replace the folded dog-ear cutout with arbitrary shapes.
* **DON'T** apply rainbow or unapproved gradients to the pillars.

---

## 7. Asset File Directory

> [!NOTE]
> The source graphics, SVG masters, and testing boards are maintained locally in the design workspace directory `assets/branding/` (which is kept untracked and excluded from git commits).
>
> The production application icons and web assets are committed directly into their respective project folders:
> * Desktop app icons: [`apps/desktop/src-tauri/icons/`](file:///home/almaver/orca/workspaces/sha-print/feat-logo/apps/desktop/src-tauri/icons)
> * Webview and PWA assets: [`apps/desktop/public/`](file:///home/almaver/orca/workspaces/sha-print/feat-logo/apps/desktop/public)

The structure of the local design workspace `assets/branding/`:

```
assets/branding/
├── dist/
│   ├── shaprint-symbol.svg                     # Master Two-Tone Symbol (100/100)
│   ├── shaprint-symbol-black.svg               # Pure Black Monochrome
│   ├── shaprint-symbol-white.svg               # Pure White (Reversed)
│   ├── shaprint-symbol-mono-teal.svg           # Spruce Teal Monochrome
│   ├── shaprint-symbol-mono-slate.svg          # Dark Slate Monochrome
│   ├── shaprint-symbol-app-icon.svg            # App Icon squircle tile (Dark)
│   ├── shaprint-symbol-app-icon-teal.svg       # App Icon squircle tile (Teal)
│   ├── shaprint-symbol-favicon.svg             # Tight-crop favicon SVG
│   ├── shaprint-lockup-horizontal.svg          # Master Outlined Horizontal Lockup
│   ├── shaprint-lockup-horizontal-black.svg    # Black Outlined Horizontal Lockup
│   ├── shaprint-lockup-horizontal-white.svg    # White Outlined Horizontal Lockup
│   ├── shaprint-lockup-stacked.svg             # Master Outlined Stacked Lockup
│   ├── shaprint-symbol-[16..1024].png          # High-resolution raster ladder
│   ├── favicon.ico                             # Web favicon (16/32/48 px)
│   ├── icon.ico                                # Windows app icon
│   └── site.webmanifest                        # PWA manifest
├── preview.html                                # Interactive browser test sheet
├── preview_sheet.png                           # Comprehensive QA visual board
└── secure_document_concepts.png                # Selection presentation sheet
```
