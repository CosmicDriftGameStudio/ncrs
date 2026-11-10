// Renders the trailer: the app pictures from `cargo test -p ncrs trailer_frames -- --ignored`
// (target/trailer/timeline.json) get camera moves, titles, key caps and cross-fades on an HTML
// stage (stage.html, one render(t) function), captured frame by frame into ffmpeg.
// Outputs in target/trailer/out/: ncrs-trailer.mp4, web/en.mp4, web/en.jpg, social.mp4, appstore.mp4.
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { chromium } from "playwright";
import { AI_NOTICE, CAST, CREDITS, END_SAY, story, WALL, WALL_SAY, WALL_STAMPS, type Camera, type Stamp } from "./story";

type Step = { file: string; seconds: number; key?: string };
type Scene = { id: string; title: string; subtitle: string; steps: Step[] };
type Item =
  | { type: "title" | "ident"; dur: number }
  | { type: "credits"; dur: number; cast: string[][]; entries: string[][]; notice: string }
  | { type: "end"; id: "end"; dur: number; say: string; voice: number; logo: boolean; links: boolean; notice: string }
  | { type: "wall"; id: "wall"; dur: number; say: string; voice: number; stamps: Stamp[]; entries: string[][]; frames: string[]; drift: number }
  | { type: "scene"; id: string; title: string; subtitle: string; steps: Step[]; say: string; voice: number; stamps: Stamp[]; camera: Camera };

const FPS = 30;
const WIDE_CAMERA: Camera = { cx: 960, cy: 540, zoom: 1 };
const TITLE_SECONDS = 2.5;
const IDENT_SECONDS = 3.0;
const IDENT_MUSIC_FROM_S = 0.5;
const IDENT_MUSIC_FADE = 0.8;
const END_MIN_SECONDS = 4;
const CREDITS_SECONDS = 10;
const WALL_MIN_SECONDS = 4;
const WALL_DRIFT_PER_SECOND = 190;
const SOCIAL_SCENES = ["f1", "copy", "search"];
const SOCIAL_STEP_LIMIT = 0.8;
const POSTER_SCENE = "panels";
const VOICE_DELAY = 0.15;
const VOICE_TAIL = 0.35;
const DUCKED_MUSIC = 0.35;
const WEB_COMMENT = `${AI_NOTICE} (Google Gemini TTS, Google Lyria)`;

const root = resolve(import.meta.dir, "../..");
const work = join(root, "target/trailer");
const out = join(work, "out");
const music = join(import.meta.dir, "music.mp3");
const identMusic = join(import.meta.dir, "music-ident.mp3");
const voiceCache = join(import.meta.dir, "voice");
const voiceWork = join(work, "voice");
mkdirSync(voiceCache, { recursive: true });
mkdirSync(voiceWork, { recursive: true });
const hasMusic = existsSync(music);
mkdirSync(join(out, "web"), { recursive: true });

const scenes: Scene[] = JSON.parse(readFileSync(join(work, "timeline.json"), "utf8"));
/** Voice-over as 24 kHz mono PCM, cached in scripts/trailer/voice/ (only a missing file calls the API). Returns seconds. */
async function speak(id: string, text: string): Promise<number> {
  const pcm = join(voiceCache, `${id}.pcm`);
  if (!existsSync(pcm)) {
    const key = process.env.OPENROUTER_API_KEY;
    if (!key) throw new Error(`voice/${id}.pcm is missing and OPENROUTER_API_KEY is not set`);
    const res = await fetch("https://openrouter.ai/api/v1/audio/speech", {
      method: "POST",
      headers: { Authorization: `Bearer ${key}`, "Content-Type": "application/json" },
      body: JSON.stringify({
        model: "google/gemini-3.8-flash-tts",
        input: text,
        voice: "Kore",
        response_format: "pcm",
        instructions: "Female Hollywood action movie trailer voice-over: confident, intense, fast, high energy, hard emphasis on key words, no slow pauses.",
      }),
    });
    if (!res.ok) throw new Error(`tts ${id}: ${res.status} ${await res.text()}`);
    writeFileSync(pcm, new Uint8Array(await res.arrayBuffer()));
  }
  ffmpeg(["-f", "s16le", "-ar", "24000", "-ac", "1", "-i", pcm, join(voiceWork, `${id}.wav`)]);
  return readFileSync(pcm).byteLength / 2 / 24000;
}

const voiceLength = new Map<string, number>();
voiceLength.set("end", await speak("end", END_SAY));
voiceLength.set("wall", await speak("wall", WALL_SAY));
for (const scene of scenes) {
  const say = story[scene.id]?.say;
  if (say) voiceLength.set(scene.id, await speak(scene.id, say));
}

const sceneById = (id: string): Scene => {
  const scene = scenes.find((candidate) => candidate.id === id);
  if (!scene) throw new Error(`unknown scene "${id}" (known: ${scenes.map((s) => s.id).join(", ")})`);
  return scene;
};
const sceneItem = (scene: Scene, stepLimit = Infinity): Item => {
  const steps = scene.steps.map((step) => ({ ...step, seconds: Math.min(step.seconds, stepLimit) }));
  for (const st of story[scene.id]?.stamps ?? []) {
    if (st.step >= steps.length || st.step < -steps.length) throw new Error(`story ${scene.id}: stamp step ${st.step} out of range`);
  }
  const voice = voiceLength.get(scene.id) ?? 0;
  const lack = VOICE_DELAY + voice + VOICE_TAIL - steps.reduce((sum, step) => sum + step.seconds, 0);
  if (voice > 0 && lack > 0) steps[steps.length - 1].seconds += lack;
  return { type: "scene", id: scene.id, title: scene.title, subtitle: scene.subtitle, steps, say: story[scene.id]?.say ?? "", voice, stamps: story[scene.id]?.stamps ?? [], camera: story[scene.id]?.camera ?? WIDE_CAMERA };
};
const endItem = (logo: boolean, links = true): Item => {
  const voice = voiceLength.get("end") ?? 0;
  return { type: "end", id: "end", dur: Math.max(END_MIN_SECONDS, VOICE_DELAY + voice + VOICE_TAIL + 0.5), say: END_SAY, voice, logo, links, notice: AI_NOTICE };
};
/** One small still per scene (its middle step) for the shortcut wall. */
function wallFrames(): string[] {
  const dir = join(work, "wall");
  mkdirSync(dir, { recursive: true });
  return scenes.map((scene) => {
    const file = scene.steps[Math.floor(scene.steps.length / 2)].file;
    const small = join(dir, `${scene.id}.jpg`);
    ffmpeg(["-i", join(work, file), "-vf", "scale=600:-1", "-q:v", "4", small]);
    return `wall/${scene.id}.jpg`;
  });
}
let wallCache: Item | undefined;
const wallItem = (): Item => {
  if (wallCache) return wallCache;
  const voice = voiceLength.get("wall") ?? 0;
  const dur = Math.max(WALL_MIN_SECONDS, VOICE_DELAY + voice + VOICE_TAIL);
  wallCache = { type: "wall", id: "wall", dur, say: WALL_SAY, voice, stamps: WALL_STAMPS, entries: WALL, frames: wallFrames(), drift: WALL_DRIFT_PER_SECOND * dur };
  return wallCache;
};
const creditsItem = (): Item => ({ type: "credits", dur: CREDITS_SECONDS, cast: CAST, entries: CREDITS, notice: AI_NOTICE });
const itemSeconds = (item: Item) => (item.type === "scene" ? item.steps.reduce((s, step) => s + step.seconds, 0) : item.dur);
const spoken = (item: Item): item is Extract<Item, { voice: number }> => (item.type === "scene" || item.type === "end" || item.type === "wall") && item.voice > 0;
const seconds = (items: Item[]) => items.reduce((sum, item) => sum + itemSeconds(item), 0);

/** Voice lines of a cut mixed onto one track; returns the file and the time windows where the voice is audible. */
function voiceTrack(items: Item[], target: string) {
  const inputs: string[] = [];
  const filters: string[] = [];
  const windows: [number, number][] = [];
  let at = 0;
  for (const item of items) {
    if (spoken(item)) {
      const from = at + VOICE_DELAY;
      filters.push(`[${windows.length}:a]adelay=${Math.round(from * 1000)}:all=1[v${windows.length}]`);
      inputs.push("-i", join(voiceWork, `${item.id}.wav`));
      windows.push([from, from + item.voice]);
    }
    at += itemSeconds(item);
  }
  if (filters.length === 0) return null;
  const labels = filters.map((_, n) => `[v${n}]`).join("");
  ffmpeg([...inputs, "-filter_complex", `${filters.join(";")};${labels}amix=inputs=${filters.length}:normalize=0[a]`, "-map", "[a]", target]);
  return { file: target, windows };
}

/** Sentences of each scene's line, timed by character count across the voice (close enough for captions). */
function writeCaptions(items: Item[], target: string) {
  const stamp = (s: number) => new Date(Math.round(s * 1000)).toISOString().slice(11, 23);
  const cues: string[] = [];
  let at = 0;
  for (const item of items) {
    if (spoken(item)) {
      let done = 0;
      for (const sentence of item.say.match(/[^.!?]+[.!?]+/g) ?? [item.say]) {
        const from = at + VOICE_DELAY + (done / item.say.length) * item.voice;
        done += sentence.length;
        cues.push(`${stamp(from)} --> ${stamp(at + VOICE_DELAY + (done / item.say.length) * item.voice)}\n${sentence.trim()}`);
      }
    }
    at += itemSeconds(item);
  }
  writeFileSync(target, `WEBVTT\n\n${cues.join("\n\n")}\n`);
}

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

/**
 * Mixes the voice over the music (music ducked while the voice plays). A cut that opens with the ident gets the
 * ident sting first and the trailer music starting as the sting fades out.
 */
function finish(silent: string, target: string, length: number, items: Item[], voice: { file: string; windows: [number, number][] } | null, video: string[], extra: string[] = []) {
  const lead = items[0].type === "ident" ? items[0].dur : 0;
  const duck = (voice?.windows ?? []).map(([from, to]) => `volume=${DUCKED_MUSIC}:enable='between(t,${from.toFixed(3)},${to.toFixed(3)})'`).join(",");
  const musicStart = Math.max(0, lead - 0.4);
  const musicChain = `adelay=${Math.round(musicStart * 1000)}:all=1,afade=t=in:st=${musicStart.toFixed(3)}:d=0.8,afade=t=out:st=${(length - 1.5).toFixed(3)}:d=1.5${duck ? "," + duck : ""},apad`;
  const inputs = ["-i", silent];
  const lastInput = () => inputs.filter((x) => x === "-i").length - 1;
  const filters: string[] = [];
  const mixed: string[] = [];
  if (voice) {
    inputs.push("-i", voice.file);
    mixed.push(`[${lastInput()}:a]`);
  }
  if (hasMusic) {
    inputs.push("-i", music);
    filters.push(`[${lastInput()}:a]${musicChain}[m]`);
    mixed.push("[m]");
  }
  if (lead > 0 && existsSync(identMusic)) {
    inputs.push("-ss", IDENT_MUSIC_FROM_S.toString(), "-t", lead.toFixed(3), "-i", identMusic);
    filters.push(`[${lastInput()}:a]afade=t=in:d=0.3,afade=t=out:st=${(lead - IDENT_MUSIC_FADE).toFixed(3)}:d=${IDENT_MUSIC_FADE}[id]`);
    mixed.push("[id]");
  }
  filters.push(`${mixed.join("")}amix=inputs=${mixed.length}:duration=longest:normalize=0[a]`);
  ffmpeg([...inputs, "-filter_complex", filters.join(";"), "-map", "0:v", "-map", "[a]", ...video, "-c:a", "aac", "-ar", "44100", "-ac", "2", "-b:a", "192k", "-t", length.toFixed(3), "-movflags", "+faststart", ...extra, target]);
}

console.log(hasMusic ? `music: ${music}` : "no scripts/trailer/music.mp3: rendering silent (bun scripts/trailer/generate-music.ts music)");

const only = process.env.TRAILER_SCENES;
if (only) {
  const wanted = only.split(",");
  const cut = wanted.flatMap((id): Item[] =>
    id === "ident" ? [{ type: "ident", dur: IDENT_SECONDS }] : id === "wall" ? [wallItem()] : id === "end" ? [endItem(false), creditsItem()] : [sceneItem(sceneById(id))],
  );
  const length = await capture(cut, join(work, "test.silent.mp4"));
  finish(join(work, "test.silent.mp4"), join(out, "test.mp4"), length, cut, voiceTrack(cut, join(voiceWork, "test.wav")), ["-c:v", "copy"]);
  await browser.close();
  console.log(`test.mp4 ${seconds(cut).toFixed(1)} s`);
  process.exit(0);
}
// no title card in the hero cut: the ident leads straight into the first scene and the end card names the app
const hero: Item[] = [
  { type: "ident", dur: IDENT_SECONDS },
  ...scenes.map((scene) => sceneItem(scene)),
  wallItem(),
  endItem(false),
  creditsItem(),
];
const social: Item[] = [
  { type: "title", dur: 1.4 },
  ...SOCIAL_SCENES.map((id) => sceneItem(sceneById(id), SOCIAL_STEP_LIMIT)),
  wallItem(),
  endItem(true),
];
const heroLength = await capture(hero, join(work, "hero.silent.mp4"));
const heroVoice = voiceTrack(hero, join(voiceWork, "hero.wav"));
finish(join(work, "hero.silent.mp4"), join(out, "ncrs-trailer.mp4"), heroLength, hero, heroVoice, ["-c:v", "copy"]);
writeCaptions(hero, join(out, "web/en.vtt"));
finish(join(work, "hero.silent.mp4"), join(out, "web/en.mp4"), heroLength, hero, heroVoice, ["-c:v", "libx264", "-crf", "25", "-pix_fmt", "yuv420p"], ["-metadata", `comment=${WEB_COMMENT}`]);

const socialLength = await capture(social, join(work, "social.silent.mp4"));
finish(join(work, "social.silent.mp4"), join(out, "social.mp4"), socialLength, social, voiceTrack(social, join(voiceWork, "social.wav")), ["-c:v", "copy"]);

// Mac App Store preview: the social cut without the URL and the other download channels (Apple rejects
// previews that name other platforms); Apple asks for 256 kbit/s AAC
const appStore = social.map((item) => (item.type === "end" ? endItem(true, false) : item));
const appStoreLength = await capture(appStore, join(work, "appstore.silent.mp4"));
finish(join(work, "appstore.silent.mp4"), join(out, "appstore.mp4"), appStoreLength, appStore, voiceTrack(appStore, join(voiceWork, "appstore.wav")), ["-c:v", "copy"], ["-b:a", "256k"]);

// poster: the whole window at the end of the panels scene, no title, no cap
await page.evaluate((cut) => (globalThis as any).setup(cut), hero);
const posterAt: number = await page.evaluate((id) => (globalThis as any).sceneEnd(id) - 0.05, POSTER_SCENE);
await page.evaluate(([time]) => (globalThis as any).render(time, { bare: true, wide: true }), [posterAt]);
await page.screenshot({ path: join(out, "web/en.jpg"), type: "jpeg", quality: 92 });
await browser.close();

console.log(`hero ${seconds(hero).toFixed(1)} s, social ${seconds(social).toFixed(1)} s`);
