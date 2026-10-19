import { initHeroMotion } from "./hero-motion";

initHeroMotion();

const privacyNavigator = navigator as Navigator & {
  globalPrivacyControl?: boolean;
};

// The download is a normal link. A failed or blocked counter never delays it.
if (navigator.doNotTrack !== "1" && !privacyNavigator.globalPrivacyControl) {
  document.addEventListener("click", (event) => {
    const target = event.target instanceof Element ? event.target : null;
    const link = target?.closest<HTMLAnchorElement>("a[data-download]");
    if (!link?.dataset.download) return;
    const body = JSON.stringify({ package: link.dataset.download });
    try {
      if (
        !navigator.sendBeacon?.(
          "/api/download",
          new Blob([body], { type: "application/json" }),
        )
      ) {
        void fetch("/api/download", {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body,
          keepalive: true,
          credentials: "omit",
        }).catch(() => {});
      }
    } catch {
      /* Navigation remains available even with measurement disabled. */
    }
  });
}
