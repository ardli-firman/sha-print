/** A synchronized, illustrative print journey. No print-job data enters this scene. */
export function createHeroScene(viewport: HTMLElement) {
  const tracks: { animation: Animation; svg: SVGSVGElement }[] = [];
  const duration = 9000;
  let sceneTime = 0;

  for (const svg of viewport.querySelectorAll<SVGSVGElement>(".hero-diagram")) {
    const compact = svg.classList.contains("hero-diagram-mobile");
    function animate(selector: string, frames: Keyframe[], easing = "linear") {
      const element = svg.querySelector<SVGElement>(selector);
      if (!element) return;
      const animation = element.animate(
        frames.map((frame) => ({ ...frame, easing: frame.easing ?? easing })),
        {
          duration,
          iterations: Infinity,
          fill: "both",
          easing: "linear",
        },
      );
      animation.pause();
      tracks.push({ animation, svg });
    }
    const point = (desktop: string, small: string) =>
      compact ? small : desktop;
    animate(".scene-flying-document", [
      {
        offset: 0,
        transform: point(
          "translate(139px, 136px) scale(1)",
          "translate(73px, 110px) scale(.46)",
        ),
        opacity: 0,
      },
      {
        offset: 0.08,
        transform: point(
          "translate(139px, 136px) scale(1)",
          "translate(73px, 110px) scale(.46)",
        ),
        opacity: 0,
      },
      {
        offset: 0.12,
        transform: point(
          "translate(151px, 153px) rotate(-8deg) scale(1.1)",
          "translate(78px, 150px) rotate(-8deg) scale(.8)",
        ),
        opacity: 1,
      },
      {
        offset: 0.29,
        transform: point(
          "translate(147px, 336px) rotate(6deg) scale(1)",
          "translate(85px, 232px) rotate(6deg) scale(.72)",
        ),
        opacity: 1,
      },
      {
        offset: 0.36,
        transform: point(
          "translate(166px, 371px) scale(.1)",
          "translate(93px, 278px) scale(.1)",
        ),
        opacity: 0,
      },
      {
        offset: 0.43,
        transform: point(
          "translate(202px, 362px) scale(.1)",
          "translate(140px, 278px) scale(.1)",
        ),
        opacity: 0,
      },
      {
        offset: 0.49,
        transform: point(
          "translate(252px, 336px) rotate(-10deg) scale(1)",
          "translate(170px, 245px) rotate(-10deg) scale(.8)",
        ),
        opacity: 1,
      },
      {
        offset: 0.62,
        transform: point(
          "translate(438px, 270px) rotate(6deg) scale(.9)",
          "translate(275px, 250px) rotate(6deg) scale(.7)",
        ),
        opacity: 1,
      },
      {
        offset: 0.67,
        transform: point(
          "translate(474px, 292px) scale(.1)",
          "translate(296px, 286px) scale(.1)",
        ),
        opacity: 0,
      },
      {
        offset: 1,
        transform: point(
          "translate(474px, 292px) scale(.1)",
          "translate(296px, 286px) scale(.1)",
        ),
        opacity: 0,
      },
    ]);
    animate(".scene-phone-device", [
      { offset: 0, transform: "translateY(0) rotate(0deg)" },
      { offset: .14, transform: "translateY(0) rotate(0deg)" },
      { offset: .21, transform: "translateY(-10px) rotate(3deg)" },
      { offset: .38, transform: "translateY(0) rotate(0deg)" },
      { offset: 1, transform: "translateY(0) rotate(0deg)" },
    ], "ease-in-out");
    animate(".scene-phone-send", [
      { offset: 0, stroke: "#126b73" },
      { offset: .15, stroke: "#126b73" },
      { offset: .21, stroke: "#249ea0" },
      { offset: .35, stroke: "#126b73" },
      { offset: 1, stroke: "#126b73" },
    ]);
    animate(".scene-phone-document", [
      { offset: 0, opacity: 0, transform: point("translate(294px, 188px) scale(.6)", "translate(201px, 145px) scale(.4)") },
      { offset: .17, opacity: 0, transform: point("translate(294px, 188px) scale(.6)", "translate(201px, 145px) scale(.4)") },
      { offset: .22, opacity: 1, transform: point("translate(280px, 226px) rotate(9deg) scale(.9)", "translate(197px, 184px) rotate(9deg) scale(.7)") },
      { offset: .32, opacity: 1, transform: point("translate(207px, 299px) rotate(-7deg) scale(.85)", "translate(120px, 224px) rotate(-7deg) scale(.65)") },
      { offset: .4, opacity: 0, transform: point("translate(166px, 371px) scale(.1)", "translate(93px, 278px) scale(.1)") },
      { offset: 1, opacity: 0, transform: point("translate(166px, 371px) scale(.1)", "translate(93px, 278px) scale(.1)") },
    ]);
    animate(".scene-client-preview", [
      { offset: 0, opacity: 1 },
      { offset: 0.08, opacity: 1 },
      { offset: 0.13, opacity: 0 },
      { offset: 0.88, opacity: 0 },
      { offset: 0.98, opacity: 1 },
      { offset: 1, opacity: 1 },
    ]);
    animate(
      ".scene-client-device",
      [
        { offset: 0, transform: "translateY(0)" },
        { offset: 0.12, transform: "translateY(-12px)" },
        { offset: 0.3, transform: "translateY(0)" },
        { offset: 0.84, transform: "translateY(0)" },
        { offset: 1, transform: "translateY(0)" },
      ],
      "ease-in-out",
    );
    animate(
      ".scene-server-device",
      [
        { offset: 0, transform: "translateY(0) scale(1)" },
        { offset: 0.24, transform: "translateY(0) scale(1)" },
        { offset: 0.33, transform: "translateY(-8px) scale(1.08)" },
        { offset: 0.47, transform: "translateY(0) scale(1)" },
        { offset: 1, transform: "translateY(0) scale(1)" },
      ],
      "ease-in-out",
    );
    animate(".scene-arrival-ring", [
      { offset: 0, transform: "scale(.8)", opacity: 0 },
      { offset: 0.25, transform: "scale(.8)", opacity: 0 },
      { offset: 0.31, transform: "scale(1)", opacity: 0.55 },
      { offset: 0.46, transform: "scale(1.4)", opacity: 0 },
      { offset: 1, transform: "scale(1.4)", opacity: 0 },
    ]);
    animate(".scene-processing-ring", [
      { offset: 0, transform: "rotate(0deg)", opacity: 0 },
      { offset: 0.24, transform: "rotate(0deg)", opacity: 1 },
      { offset: 0.46, transform: "rotate(540deg)", opacity: 1 },
      { offset: 0.5, transform: "rotate(600deg)", opacity: 0 },
      { offset: 1, transform: "rotate(600deg)", opacity: 0 },
    ]);
    animate(".scene-server-check", [
      { offset: 0, opacity: 0 },
      { offset: 0.47, opacity: 0 },
      { offset: 0.51, opacity: 1 },
      { offset: 0.88, opacity: 1 },
      { offset: 0.97, opacity: 0 },
      { offset: 1, opacity: 0 },
    ]);
    animate(
      ".scene-printer-body",
      [
        { offset: 0, transform: "translateY(0)" },
        { offset: 0.59, transform: "translateY(0)" },
        { offset: 0.64, transform: "translateY(-5px)" },
        { offset: 0.69, transform: "translateY(2px)" },
        { offset: 0.74, transform: "translateY(-2px)" },
        { offset: 0.8, transform: "translateY(0)" },
        { offset: 1, transform: "translateY(0)" },
      ],
      "ease-in-out",
    );
    animate(
      ".scene-printed-sheet",
      [
        { offset: 0, transform: "translateY(-145px)", opacity: 1 },
        { offset: 0.65, transform: "translateY(-145px)", opacity: 1 },
        { offset: 0.83, transform: "translateY(0)", opacity: 1 },
        { offset: 0.88, transform: "translateY(12px)", opacity: 1 },
        { offset: 0.96, transform: "translateY(12px)", opacity: 1 },
        { offset: 1, transform: "translateY(38px)", opacity: 0 },
      ],
      "ease-in-out",
    );
    animate(".scene-paper-check", [
      { offset: 0, transform: "translateY(0)", opacity: 0 },
      { offset: 0.83, transform: "translateY(0)", opacity: 0 },
      { offset: 0.88, transform: "translateY(12px)", opacity: 1 },
      { offset: 0.96, transform: "translateY(12px)", opacity: 1 },
      { offset: 1, transform: "translateY(38px)", opacity: 0 },
    ]);
    animate(".scene-printer-light", [
      { offset: 0, fill: "#126b73" },
      { offset: 0.58, fill: "#126b73" },
      { offset: 0.65, fill: "#249ea0" },
      { offset: 0.8, fill: "#249ea0" },
      { offset: 0.9, fill: "#126b73" },
      { offset: 1, fill: "#126b73" },
    ]);
    animate(".scene-phase-send", [
      { offset: 0, opacity: 1 },
      { offset: 0.25, opacity: 1 },
      { offset: 0.29, opacity: 0 },
      { offset: 1, opacity: 0 },
    ]);
    animate(".scene-phase-route", [
      { offset: 0, opacity: 0 },
      { offset: 0.25, opacity: 0 },
      { offset: 0.29, opacity: 1 },
      { offset: 0.62, opacity: 1 },
      { offset: 0.66, opacity: 0 },
      { offset: 1, opacity: 0 },
    ]);
    animate(".scene-phase-print", [
      { offset: 0, opacity: 0 },
      { offset: 0.62, opacity: 0 },
      { offset: 0.66, opacity: 1 },
      { offset: 0.96, opacity: 1 },
      { offset: 1, opacity: 0 },
    ]);
    animate(".scene-progress", [
      { transform: "scaleX(0)" },
      { transform: "scaleX(1)" },
    ]);
  }

  return {
    sync(running: boolean, reduced: boolean) {
      const advancing = tracks.find(
        ({ animation }) => animation.playState === "running",
      );
      if (typeof advancing?.animation.currentTime === "number")
        sceneTime = advancing.animation.currentTime;
      for (const { animation, svg } of tracks) {
        if (reduced) {
          animation.cancel();
          continue;
        }
        animation.pause();
        animation.currentTime = sceneTime;
        if (running && getComputedStyle(svg).display !== "none")
          animation.play();
      }
    },
  };
}
