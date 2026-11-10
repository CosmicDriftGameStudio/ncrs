// Voice-over line and comic stamp words per scene (scene ids from target/trailer/timeline.json).
// `step` indexes the scene's steps; a stamp slams in at that step's start. Negative counts from the end.
export type Stamp = { text: string; step: number; at?: number; left: number; top: number; size?: number; rot?: number; color?: string };
/** Hand-picked camera focus in logical 1920x1080 px. */
export type Camera = { cx: number; cy: number; zoom: number };
export type SceneStory = { say: string; stamps: Stamp[]; camera: Camera };

const AMBER = "#f59e0b";

export const story: Record<string, SceneStory> = {
  panels: {
    camera: { cx: 960, cy: 540, zoom: 1.08 },
    say: "Two panels. Your keyboard does the rest.",
    stamps: [
      { text: "TWO PANELS.", step: 0, left: 90, top: 720, size: 150 },
      { text: "NO MOUSE.", step: 6, left: 1010, top: 720, size: 150, rot: 3 },
    ],
  },
  f1: {
    camera: { cx: 768, cy: 540, zoom: 1.25 },
    say: "Forgot a key? Press F1.",
    stamps: [
      { text: "FORGOT A KEY?", step: 0, left: 90, top: 760, size: 120 },
      { text: "F1.", step: 1, left: 1350, top: 640, size: 260, color: AMBER },
    ],
  },
  copy: {
    camera: { cx: 768, cy: 520, zoom: 1.25 },
    say: "Tag. Copy. Done.",
    stamps: [
      { text: "TAG.", step: 1, left: 90, top: 700, size: 150 },
      { text: "COPY.", step: 5, left: 90, top: 840, size: 150 },
      { text: "DONE.", step: 6, left: 1250, top: 760, size: 150, color: AMBER },
    ],
  },
  favourite: {
    camera: { cx: 768, cy: 432, zoom: 1.25 },
    say: "Your folders on Command one to five.",
    stamps: [{ text: "⌘1–5", step: 1, left: 90, top: 760, size: 170 }],
  },
  zip: {
    camera: { cx: 1152, cy: 540, zoom: 1.25 },
    say: "Archives? Just folders.",
    stamps: [
      { text: "ZIP?", step: 0, left: 90, top: 720, size: 150 },
      { text: "JUST A FOLDER.", step: 1, left: 90, top: 860, size: 130, color: AMBER },
    ],
  },
  "batch-rename": {
    camera: { cx: 960, cy: 432, zoom: 1.25 },
    say: "Rename a hundred files. See it before it happens.",
    stamps: [
      { text: "×100", step: 2, left: 90, top: 720, size: 170 },
      { text: "PREVIEW FIRST.", step: 6, left: 90, top: 860, size: 130, color: AMBER },
    ],
  },
  search: {
    camera: { cx: 768, cy: 540, zoom: 1.25 },
    say: "Find anything. Names and contents.",
    stamps: [{ text: "FOUND.", step: 2, left: 90, top: 760, size: 170 }],
  },
  compare: {
    camera: { cx: 960, cy: 540, zoom: 1.10 },
    say: "Two folders. Newer, missing, different.",
    stamps: [
      { text: "NEWER.", step: 3, left: 90, top: 700, size: 130 },
      { text: "MISSING.", step: 4, left: 90, top: 840, size: 130 },
      { text: "DIFFERENT.", step: 5, left: 900, top: 770, size: 130, color: AMBER },
    ],
  },
  sftp: {
    camera: { cx: 768, cy: 432, zoom: 1.25 },
    say: "Servers are just folders.",
    stamps: [{ text: "SFTP.", step: -1, left: 90, top: 760, size: 170 }],
  },
  themes: {
    camera: { cx: 960, cy: 540, zoom: 1.05 },
    say: "Make it yours.",
    stamps: [{ text: "YOUR COLOURS.", step: 5, left: 90, top: 760, size: 140 }],
  },
};

export const END_SAY = "ncrs. Two panels. Every key.";
export const AI_NOTICE = "Voice and music AI-generated";
/** Role, name, optional note. */
export const CREDITS: string[][] = [
  ["An idea by", "Marc"],
  ["Cut by", "Claude", "who also wrote the code. And this line."],
  ["Starring", "ncrs", "as the file manager that never needed a mouse"],
  ["Voice", "Kore", "one take. Again."],
  ["Every frame", "The real UI", "no mockups were harmed"],
  ["Catering", "Coffee", "still a lot of it"],
];

/** Film-credit cast: key, role, optional note. Every key is a default binding on macOS (keymap/defaults.rs); Esc is the reserved key that closes dialogs and stops jobs. */
export const CAST: string[][] = [
  ["F5", "Copy"],
  ["F6", "Move"],
  ["⇥ Tab", "The other panel"],
  ["Space", "Tag"],
  ["F1", "The one who remembers everything"],
  ["Esc", "The way out"],
  ["The mouse", "Himself", "cameo, cut"],
];

export const WALL_SAY = "Over 60 actions. Every one on a key.";
export const WALL_STAMPS: Stamp[] = [
  { text: "60+ ACTIONS.", step: 0, at: 0.3, left: 140, top: 250, size: 190, rot: -5 },
  { text: "ALL ON KEYS.", step: 0, at: 1.7, left: 560, top: 600, size: 210, rot: -3, color: AMBER },
];
/** The default macOS bindings that are not Hermes, filing, AI or OCR: key label, action label (strings.json). 62 of them. */
export const WALL: string[][] = [
  ["⌘Q", "Quit"],
  ["⌫", "Go to parent folder"],
  ["↩", "Open"],
  ["⇥", "Switch panel"],
  ["⌃⇥", "Next tab"],
  ["⌃⇧⇥", "Previous tab"],
  ["←", "Activate left panel"],
  ["→", "Activate right panel"],
  ["F9", "Switch language"],
  ["Space", "Tag or untag row"],
  ["⇧↓", "Tag and move down"],
  ["⇧↑", "Tag and move up"],
  ["*", "Tag all rows"],
  ["⌃*", "Untag all rows"],
  ["⌘C", "Copy files to the clipboard"],
  ["⌥⌘C", "Copy path to clipboard"],
  ["⌃⌘C", "Copy name to clipboard"],
  ["⌘V", "Paste files, image or text"],
  ["F3", "View file"],
  ["F4", "Edit file"],
  ["⌥⌘T", "Open terminal here"],
  ["F5", "Copy to other panel"],
  ["F6", "Move to other panel"],
  ["⇧F6", "Rename"],
  ["F8", "Delete"],
  ["⇧F8", "Delete permanently"],
  ["⌘Z", "Restore the last deletion"],
  ["F7", "Create folder"],
  ["⇧F4", "New file"],
  ["⌥F5", "Pack into archive"],
  ["⌥F1", "Drive menu, left panel"],
  ["⌥F2", "Drive menu, right panel"],
  ["⌥←", "Back"],
  ["⌥→", "Forward"],
  ["⌘D", "Add folder to favourites"],
  ["⌘1", "Open favourite 1"],
  ["⌘2", "Open favourite 2"],
  ["⌘3", "Open favourite 3"],
  ["⌘4", "Open favourite 4"],
  ["⌘5", "Open favourite 5"],
  ["⌥⌘1", "Sort by name"],
  ["⌥⌘2", "Sort by size"],
  ["⌥⌘3", "Sort by modification time"],
  ["⇧F10", "Context menu"],
  ["↑", "Cursor up"],
  ["↓", "Cursor down"],
  ["PgUp", "Page up"],
  ["PgDn", "Page down"],
  ["Home", "First row"],
  ["End", "Last row"],
  ["⌘⇧F", "Search in files"],
  ["⌘P", "Go to file"],
  ["⇧F2", "Go to the command line"],
  ["F1", "Command list"],
  ["⌘,", "Settings"],
  ["F12", "Monitor"],
  ["⌘K", "Keyboard shortcuts"],
  ["⌥C", "Search: match case"],
  ["⌥W", "Search: whole words"],
  ["⌥R", "Search: regular expression"],
  ["⌃C", "Stop running job"],
  ["⌥⌘R", "Permissions"],
];
