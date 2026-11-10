// Renders the trailer: the app pictures from `cargo test -p ncrs trailer_frames -- --ignored`
// (target/trailer/timeline.json) get camera moves, titles, key caps and cross-fades on an HTML
// stage (stage.html, one render(t) function), captured frame by frame into ffmpeg.
// Outputs in target/trailer/out/: ncrs-trailer.mp4, web/en.mp4, web/en.jpg, social.mp4.
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { chromium } from "playwright";

type Step = { file: string; seconds: number; key?: string; focus: unknown };
type Scene = { id: string; title: string; subtitle: string; steps: Step[] };
type Item =
  | { type: "title" | "credits"; dur: number }
  | { type: "scene"; id: string; title: string; subtitle: string; steps: Step[] };

const FPS = 30;
const TITLE_SECONDS = 2.5;
const CREDITS_SECONDS = 3.5;
const SOCIAL_SCENES = ["f1", "copy", "search"];
const SOCIAL_STEP_LIMIT = 0.8;
const POSTER_SCENE = "panels";
const WEB_COMMENT = "Music generated with AI (Google Lyria)";

const root = resolve(import.meta.dir, "../..");
const work = join(root, "target/trailer");
const out = join(work, "out");
const music = join(import.meta.dir, "music.mp3");
const hasMusic = existsSync(music);
mkdirSync(join(out, "web"), { recursive: true });

const scenes: Scene[] = JSON.parse(readFileSync(join(work, "timeline.json"), "utf8"));
const sceneItem = (scene: Scene, stepLimit = Infinity): Item => ({
  type: "scene",
  id: scene.id,
  title: scene.title,
  subtitle: scene.subtitle,
  steps: scene.steps.map((step) => ({ ...step, seconds: Math.min(step.seconds, stepLimit) })),
});
const seconds = (items: Item[]) =>
  items.reduce((sum, item) => sum + (item.type === "scene" ? item.steps.reduce((s, step) => s + step.seconds, 0) : item.dur), 0);

const hero: Item[] = [
  { type: "title", dur: TITLE_SECONDS },
  ...scenes.map((scene) => sceneItem(scene)),
  { type: "credits", dur: CREDITS_SECONDS },
];
const social: Item[] = [
  { type: "title", dur: 1.4 },
  ...SOCIAL_SCENES.map((id) => sceneItem(scenes.find((scene) => scene.id === id)!, SOCIAL_STEP_LIMIT)),
  { type: "credits", dur: 2.5 },
];

function ffmpeg(args: string[]) {
  const run = Bun.spawnSync(["ffmpeg", "-y", "-hide_banner", "-loglevel", "error", ...args], { stderr: "inherit" });
  if (run.exitCode !== 0) throw new Error(`ffmpeg ${args.join(" ")} failed`);
}

writeFileSync(
  join(work, "stage.html"),
  readFileSync(join(import.meta.dir, "stage.html"), "utf8").replaceAll("__ROOT__", `file://${root}`),
);
const browser = await chromium.launch();
const page = await browser.newPage({ viewport: { width: 1920, height: 1080 } });
await page.goto(`file://${work}/stage.html`);
await page.evaluate(() => document.fonts.ready);

async function capture(items: Item[], target: string) {
  const total: number = await page.evaluate((cut) => (globalThis as any).setup(cut), items);
  const encoder = Bun.spawn(
    ["ffmpeg", "-y", "-hide_banner", "-loglevel", "error", "-f", "image2pipe", "-framerate", `${FPS}`, "-c:v", "mjpeg", "-i", "-",
      "-vf", "scale=in_range=pc:out_range=tv,format=yuv420p", "-c:v", "libx264", "-crf", "18", target],
    { stdin: "pipe", stderr: "inherit" },
  );
  const frames = Math.ceil(total * FPS);
  for (let frame = 0; frame < frames; frame++) {
    await page.evaluate((time) => (globalThis as any).render(time), Math.min(frame / FPS, total - 1e-3));
    encoder.stdin.write(await page.screenshot({ type: "jpeg", quality: 92 }));
  }
  encoder.stdin.end();
  await encoder.exited;
  return total;
}

/** Puts the music (when there is some) under `silent`, faded in 0.3 s and out 1.5 s. */
function finish(silent: string, target: string, length: number, video: string[], extra: string[] = []) {
  const sound = hasMusic
    ? ["-i", music, "-map", "0:v", "-map", "1:a", "-af", `afade=t=in:d=0.3,afade=t=out:st=${(length - 1.5).toFixed(3)}:d=1.5`, "-c:a", "aac", "-b:a", "192k", "-t", length.toFixed(3)]
    : [];
  ffmpeg(["-i", silent, ...sound, ...video, "-movflags", "+faststart", ...extra, target]);
}

console.log(hasMusic ? `music: ${music}` : "no scripts/trailer/music.mp3: rendering silent (bun scripts/trailer/generate-music.ts music)");

const only = process.env.TRAILER_SCENES;
if (only) {
  const wanted = only.split(",");
  const cut = wanted.map((id) => sceneItem(scenes.find((scene) => scene.id === id)!));
  await capture(cut, join(out, "test-camera.mp4"));
  await browser.close();
  console.log(`test-camera.mp4 ${seconds(cut).toFixed(1)} s`);
  process.exit(0);
}
const heroLength = await capture(hero, join(work, "hero.silent.mp4"));
finish(join(work, "hero.silent.mp4"), join(out, "ncrs-trailer.mp4"), heroLength, ["-c:v", "copy"]);
finish(join(work, "hero.silent.mp4"), join(out, "web/en.mp4"), heroLength, ["-c:v", "libx264", "-crf", "25", "-pix_fmt", "yuv420p"], ["-metadata", `comment=${WEB_COMMENT}`]);

const socialLength = await capture(social, join(work, "social.silent.mp4"));
finish(join(work, "social.silent.mp4"), join(out, "social.mp4"), socialLength, ["-c:v", "copy"]);

// poster: the whole window at the end of the panels scene, no title, no cap
await page.evaluate((cut) => (globalThis as any).setup(cut), hero);
const posterAt: number = await page.evaluate((id) => (globalThis as any).sceneEnd(id) - 0.05, POSTER_SCENE);
await page.evaluate(([time]) => (globalThis as any).render(time, { bare: true, wide: true }), [posterAt]);
await page.screenshot({ path: join(out, "web/en.jpg"), type: "jpeg", quality: 92 });
await browser.close();

console.log(`hero ${seconds(hero).toFixed(1)} s, social ${seconds(social).toFixed(1)} s`);
