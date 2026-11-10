// One-off Lyria call; Lyria ignores the requested length, render.sh trims and fades the result.
// Usage: OPENROUTER_API_KEY=... bun scripts/trailer/generate-music.ts music
const PROMPTS: Record<string, string> = {
  music:
    "Instrumental modern electronic trailer music, about 45 seconds, 124 BPM. Driving and energetic from the very first second: punchy drums, pulsing synth bass, bright synth arpeggios. Clean, confident, techy and fun. Ends on one clean final hit. No vocals, no orchestra, no ethnic or folk instruments, no world music.",
};
for (const [name, prompt] of Object.entries(PROMPTS).filter(([n]) =>
  process.argv.slice(2).includes(n),
)) {
  const res = await fetch("https://openrouter.ai/api/v1/chat/completions", {
    method: "POST",
    headers: {
      Authorization: `Bearer ${process.env.OPENROUTER_API_KEY}`,
      "Content-Type": "application/json",
    },
    body: JSON.stringify({
      model: "google/lyria-3-pro-preview",
      modalities: ["text", "audio"],
      stream: true,
      messages: [{ role: "user", content: prompt }],
    }),
  });
  if (!res.ok) throw new Error(`${name}: ${res.status} ${await res.text()}`);
  let b64 = "";
  for (const line of (await res.text()).split("\n")) {
    if (!line.startsWith("data: ") || line === "data: [DONE]") continue;
    const audio = JSON.parse(line.slice(6)).choices?.[0]?.delta?.audio?.data;
    if (typeof audio === "string") b64 += audio;
  }
  // an empty stream happens now and then; rerun instead of overwriting the track with silence
  if (!b64) throw new Error(`${name}: no audio in the stream, try again`);
  await Bun.write(`${import.meta.dir}/${name}.mp3`, Buffer.from(b64, "base64"));
  console.log(name, b64.length);
}
