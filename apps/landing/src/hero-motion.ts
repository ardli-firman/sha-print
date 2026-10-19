import { createHeroScene } from "./hero-scene";

/** Decorative motion is independent of download navigation and measurement. */
export function initHeroMotion(): void {
  const hero = document.querySelector<HTMLElement>(".hero");
  const viewport = hero?.querySelector<HTMLElement>(".diagram-viewport");
  const toggle = hero?.querySelector<HTMLButtonElement>(".motion-toggle");
  if (
    !hero ||
    !viewport ||
    !toggle ||
    typeof IntersectionObserver === "undefined"
  )
    return;

  const label = toggle.querySelector("span");
  const layers = [
    ...viewport.querySelectorAll<SVGElement>("[data-parallax-layer]"),
  ];
  const reducedMotion = matchMedia("(prefers-reduced-motion: reduce)");
  const finePointer = matchMedia("(hover: hover) and (pointer: fine)");
  const compact = matchMedia("(max-width: 800px)");
  const depths: Record<string, { pointer: number; scroll: number }> = {
    background: { pointer: 5, scroll: 32 },
    printer: { pointer: 9, scroll: -14 },
    server: { pointer: 4, scroll: -4 },
    clients: { pointer: 13, scroll: 12 },
    phone: { pointer: 16, scroll: 18 },
  };
  const scene = createHeroScene(viewport);
  let userPaused = false;
  let inView = false;
  let running = false;
  let frame: number | null = null;
  let targetX = 0;
  let targetY = 0;
  let currentX = 0;
  let currentY = 0;
  let bounds = { left: 0, top: 0, width: 1, height: 1, heroHeight: 1 };

  function resetLayers() {
    targetX = targetY = currentX = currentY = 0;
    for (const layer of layers) {
      layer.style.setProperty("--layer-x", "0px");
      layer.style.setProperty("--layer-y", "0px");
    }
  }

  function render() {
    frame = null;
    if (!running) return;
    currentX += (targetX - currentX) * 0.16;
    currentY += (targetY - currentY) * 0.16;
    const progress = Math.max(
      0,
      Math.min(1, window.scrollY / bounds.heroHeight),
    );
    const strength = compact.matches ? 0.45 : 1;
    for (const layer of layers) {
      const depth = depths[layer.dataset.parallaxLayer ?? ""];
      if (!depth) continue;
      layer.style.setProperty(
        "--layer-x",
        `${(currentX * depth.pointer * strength).toFixed(2)}px`,
      );
      layer.style.setProperty(
        "--layer-y",
        `${((currentY * depth.pointer * 0.65 + progress * depth.scroll) * strength).toFixed(2)}px`,
      );
    }
    // The frame loop settles to rest; scroll/pointer events wake it when needed.
    if (Math.abs(targetX - currentX) + Math.abs(targetY - currentY) > 0.01)
      queueFrame();
  }

  function queueFrame() {
    if (running && frame === null) frame = requestAnimationFrame(render);
  }

  function measure() {
    const rect = viewport!.getBoundingClientRect();
    bounds = {
      left: rect.left + window.scrollX,
      top: rect.top + window.scrollY,
      width: Math.max(1, rect.width),
      height: Math.max(1, rect.height),
      heroHeight: Math.max(1, hero!.offsetHeight),
    };
    queueFrame();
  }

  function sync() {
    const reduced = reducedMotion.matches;
    running = !reduced && !userPaused && inView && !document.hidden;
    hero!.classList.toggle("motion-enabled", !reduced);
    hero!.classList.toggle("motion-running", running);
    toggle!.hidden = reduced;
    toggle!.setAttribute("data-paused", String(userPaused));
    if (label)
      label.textContent = userPaused ? "Resume motion" : "Pause motion";
    if (!running && frame !== null) {
      cancelAnimationFrame(frame);
      frame = null;
    }
    if (reduced || userPaused) resetLayers();
    scene.sync(running, reduced);
    queueFrame();
  }

  compact.addEventListener("change", () => {
    measure();
    sync();
  });

  toggle.addEventListener("click", () => {
    userPaused = !userPaused;
    sync();
  });
  viewport.addEventListener("pointermove", (event) => {
    if (!running || !finePointer.matches || event.pointerType === "touch")
      return;
    targetX = Math.max(
      -1,
      Math.min(
        1,
        ((event.clientX + window.scrollX - bounds.left) / bounds.width - 0.5) *
          2,
      ),
    );
    targetY = Math.max(
      -1,
      Math.min(
        1,
        ((event.clientY + window.scrollY - bounds.top) / bounds.height - 0.5) *
          2,
      ),
    );
    queueFrame();
  });
  viewport.addEventListener("pointerleave", () => {
    targetX = targetY = 0;
    queueFrame();
  });
  window.addEventListener("scroll", queueFrame, { passive: true });
  window.addEventListener("resize", measure, { passive: true });
  document.addEventListener("visibilitychange", sync);
  reducedMotion.addEventListener("change", sync);
  finePointer.addEventListener("change", () => {
    targetX = targetY = 0;
    queueFrame();
  });
  window.addEventListener("pagehide", () => {
    running = false;
    scene.sync(false, reducedMotion.matches);
    hero.classList.remove("motion-running");
    if (frame !== null) cancelAnimationFrame(frame);
    frame = null;
  });
  window.addEventListener("pageshow", sync);
  new IntersectionObserver(([entry]) => {
    inView = entry?.isIntersecting ?? false;
    sync();
  }).observe(viewport);
  measure();
  void document.fonts.ready.then(measure);
  sync();
}
