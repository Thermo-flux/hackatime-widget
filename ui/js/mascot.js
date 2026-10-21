import { $ } from "./lib/dom.js";

const SIZE = 16;

/** Every expression the engine can show. "neutral" is the bare sprite; the rest are patches in themes/data.js. */
export const EXPRESSIONS = ["neutral", "happy", "focus", "sleepy", "sad", "surprised", "love", "wink", "excited", "grumpy", "thinking", "proud", "dizzy"];

/** Fallback speech lines (used by themes without their own, e.g. Classic). Every kind the engine says is listed here. */
export const DEFAULT_LINES = {
  // Moods (persistent states)
  idle: ["Ready when you are.", "Let's build something.", "What are we making?", "I'm here if you need me.", "Code awaits!", "Standing by."],
  human: ["All human hands today!", "Nice and hand-crafted.", "Artisanal code!", "Your keystrokes, your craft.", "Typed with love.", "Handmade. Respect."],
  ai: ["Teamwork with the robots!", "The AI is doing a lot today.", "Human + AI combo!", "Pair-programming with bots.", "The robots say hi.", "Don't forget to review it!"],
  session: ["Session on. You've got this!", "In the zone.", "Clock's ticking. Nice!", "Keep that flow going.", "Coding time logged!", "Look at you go."],
  focus: ["Focus mode. Shh.", "Deep work time.", "One thing at a time.", "Notifications? Never heard.", "Head down, ship it.", "I'll be quiet. Promise."],
  break: ["Break time. Stretch!", "Rest your eyes.", "Grab some water!", "Look out a window.", "Shoulders down. Breathe.", "Walk around a bit!"],
  streak: ["Look at that streak!", "Keep it going!", "Day after day!", "Streak on fire!", "Unstoppable!", "Don't break the chain!"],
  empty: ["Nothing logged yet today.", "Blank slate. Let's start!", "Haven't seen code yet.", "Ready for the first commit?", "A fresh, empty day."],
  no_key: ["Let's get you connected.", "I need your Hackatime key.", "Paste your key to begin.", "No key yet. Let's fix that.", "Connect me to Hackatime!"],
  offline: ["Can't reach Hackatime.", "Offline. I'll keep trying.", "No signal. I'll retry.", "Network's down. Retrying.", "Waiting for the network."],
  error: ["Can't reach Hackatime...", "Hmm, something's off.", "Connection hiccup.", "No signal. Retrying...", "The server's napping?", "We'll be back."],
  // Reactions (short events)
  connected: ["Connected!"],
  poke: ["Hehe!", "Boop!", "That tickles.", "Hi there!", "You rang?", "*wiggle*", "Oh, hello!", "Poke accepted."],
  pokeMore: ["Whoa, whoa!", "Okay, okay!", "Again?!", "So many pokes!", "Hey, I'm working!", "Easy there!"],
  dizzy: ["Everything's spinning...", "Too... many... pokes.", "Seeing stars!", "Whoa, dizzy...", "Which way is up?", "Need a sec..."],
  love: ["Aww, thanks!", "I like you too!", "Best human ever.", "So cozy.", "Pets accepted!", "<3"],
  cheer: ["Phase complete!", "Well done!", "Nailed it!", "Yay, you!", "That's a win!", "High five!"],
  focusHigh: ["Locked in. Great focus!", "Barely looked away!", "That was deep work.", "Laser focus!"],
  focusMid: ["Solid round.", "Decent focus.", "Good enough, nice.", "Round done."],
  focusLow: ["Distracted? Next round.", "Mostly thinking, huh?", "Try a quieter round.", "Break time. Reset."],
  pomoDone: ["Focus round done!", "Pomodoro complete!", "Earned that break!", "Look at that focus!", "One more down!", "Proud of you!"],
  sessionStart: ["Ooh, we're coding!", "Session started!", "Let's gooo!", "Here we go!", "Clock's on!"],
  sessionEnd: ["Good session!", "Nice work today.", "Session wrapped!", "That was productive!", "Time well spent."],
  longSession: ["Over an hour! Wow.", "Marathon session!", "Big session! Stretch?", "You're on a roll!", "Long haul! Hydrate."],
  loading: ["Fetching stats...", "Let me check...", "Counting minutes...", "One sec...", "Thinking..."],
  chatter: ["Nice weather in here.", "Did you commit?", "Remember to hydrate!", "I believe in you.", "*hums quietly*", "Tabs or spaces?", "Fun fact: I'm 16x16.", "Save your work!"],
  drowsy: ["Getting sleepy...", "Zzz...", "*yawn*", "Still there?", "Resting my eyes..."],
  wake: ["Huh?! I'm up!", "Oh! You're back!", "Wasn't sleeping!", "Welcome back!", "Back to it?"],
  morning: ["Good morning!", "Morning! Coffee?", "Rise and code!", "Fresh day, fresh code."],
  afternoon: ["Good afternoon!", "Afternoon push!", "Hi! Lunch was good?", "Let's keep going!"],
  evening: ["Good evening!", "Evening coding? Nice.", "Hey there, night owl.", "Cozy evening code."],
  night: ["It's late! Sleep soon?", "Midnight coder!", "The bugs come out now.", "Don't stay up too late."],
};
/** Every speech-line kind the engine uses (themes may provide any subset; the rest falls back to DEFAULT_LINES). */
export const LINE_KINDS = Object.keys(DEFAULT_LINES);

/** Expression shown for each persistent mood. */
const MOOD_EXPRESSION = { idle: "neutral", human: "happy", ai: "happy", session: "focus", focus: "focus", break: "sleepy", streak: "happy", error: "sad", empty: "neutral", no_key: "neutral", offline: "sad" };
/** Reactions: line kind -> [expression, how long to hold it (ms)]. */
const REACTIONS = {
  poke: ["happy", 1200], pokeMore: ["surprised", 1400], dizzy: ["dizzy", 2800], love: ["love", 2600],
  cheer: ["excited", 2500], connected: ["excited", 2800], pomoDone: ["proud", 3200], focusHigh: ["proud", 3200], focusMid: ["happy", 2800], focusLow: ["thinking", 3000], sessionStart: ["excited", 2200], sessionEnd: ["proud", 2600],
  longSession: ["proud", 3200], streak: ["excited", 3000], error: ["grumpy", 2600], loading: ["thinking", 1600],
  chatter: ["happy", 1600], drowsy: ["sleepy", 2000], wake: ["surprised", 1500],
  morning: ["happy", 2600], afternoon: ["happy", 2600], evening: ["wink", 2000], night: ["sleepy", 2600],
};
/** Expressions whose eyes are plain enough to blink. */
const CAN_BLINK = new Set(["neutral", "happy", "focus", "sad", "surprised", "thinking"]);
/** Moods in which the mascot may doze off / fidget (not while focusing or on errors). */
const CALM = new Set(["idle", "human", "ai", "streak", "empty"]);

const DROWSY_AFTER = 10 * 60 * 1000; // no input for 10 minutes
const LONG_SESSION = 60 * 60 * 1000;

let theme = null;
let mood = "idle";
let override = null; // temporary expression (reactions)
let overrideTimer = null;
let blinking = false;
let drowsy = false;
let loading = false;
let lastActive = Date.now();
let lastSaid = 0;
let lastText = "";
let nextFidget = Date.now() + 20000;
let pokes = [];
let pomoPrev = null;
let lastEnded = null; // the Pomodoro phase that ended most recently
let sessionSince = 0;
let longSessionSaid = false;
let everLoaded = false;
const signals = { no_key: false, offline: false, error: false, pomo: null, session: false, ai: null, streak: 0, empty: false };

/** Paint a sprite with an optional list of expression patches applied in order. */
export function draw(canvas, t, ...patchNames) {
  const ctx = canvas.getContext("2d");
  ctx.clearRect(0, 0, SIZE, SIZE);
  const grid = t.sprite.map((row) => row.split(""));
  for (const name of patchNames) for (const [x, y, ch] of t.patches?.[name] ?? []) grid[y][x] = ch;
  for (let y = 0; y < SIZE; y++) {
    for (let x = 0; x < SIZE; x++) {
      const ch = grid[y][x];
      if (ch === ".") continue;
      ctx.fillStyle = t.legend[ch] ?? "#ff00ff"; // magenta makes a missing legend entry obvious
      ctx.fillRect(x, y, 1, 1);
    }
  }
}

function expression() {
  if (override) return override;
  if (drowsy && CALM.has(mood)) return "sleepy";
  if (loading && mood !== "error" && mood !== "focus") return "thinking";
  return MOOD_EXPRESSION[mood];
}

function render() {
  if (!theme) return;
  const expr = expression();
  const bar = $("mascot-bar");
  bar.dataset.expr = expr;
  if (override) bar.dataset.react = ""; else delete bar.dataset.react;
  if (!theme.sprite) return;
  const blink = blinking && CAN_BLINK.has(expr);
  draw($("mascot"), theme, ...(expr === "neutral" ? [] : [expr]), ...(blink ? ["blink"] : []));
}

const pick = (list) => list[Math.floor(Math.random() * list.length)];

// Shuffle bags so a kind cycles through all its lines before repeating, and never says the same line twice in a row.
const bags = new Map();
function nextLine(kind) {
  const pool = theme?.lines?.[kind]?.length ? theme.lines[kind] : DEFAULT_LINES[kind] ?? DEFAULT_LINES.idle;
  const key = `${theme?.id}:${kind}`;
  let bag = bags.get(key);
  if (!bag?.length) {
    bag = [...pool].sort(() => Math.random() - 0.5);
    if (bag.length > 1 && bag[bag.length - 1] === lastText) bag.unshift(bag.pop()); // drawn from the end
    bags.set(key, bag);
  }
  return bag.pop();
}

let fadeTimer = null;
function say(kind) {
  if (!theme?.sprite) return;
  const bubble = $("mascot-say");
  lastText = nextLine(kind);
  bubble.textContent = lastText;
  bubble.classList.remove("is-faded");
  lastSaid = Date.now();
  clearTimeout(fadeTimer);
  fadeTimer = setTimeout(() => bubble.classList.add("is-faded"), 9000);
}

/** Show an expression for `ms`, optionally with a line of the given kind. */
function hold(expr, kind, ms) {
  override = expr;
  render();
  if (kind) say(kind);
  clearTimeout(overrideTimer);
  overrideTimer = setTimeout(() => { override = null; render(); }, ms);
}

/** React to an event by its line kind (e.g. "love", "dizzy", "pomoDone"). Returns false for unknown events. */
export function react(kind) {
  const r = REACTIONS[kind];
  if (!r) return false;
  hold(r[0], kind, r[1]);
  return true;
}

/** Show an expression without speaking (for previews and tests). */
export function express(expr, ms = 1500) {
  if (!EXPRESSIONS.includes(expr)) return false;
  hold(expr, null, ms);
  return true;
}

function evaluate() {
  const { no_key, offline, error, pomo, session, ai, streak, empty } = signals;
  let next = "idle";
  // Priority: no_key > offline > error > session/pomodoro > empty > what the data says > time of day.
  if (no_key) next = "no_key";
  else if (offline) next = "offline";
  else if (error) next = "error";
  else if (pomo?.running) next = pomo.phase === "focus" ? "focus" : "break";
  else if (session) next = "session";
  else if (empty) next = "empty";
  else if (ai !== null && ai > 50) next = "ai";
  else if (ai !== null) next = "human";
  else if (streak >= 3) next = "streak";
  if (next === mood) return;
  mood = next;
  $("mascot-bar").dataset.mood = mood;
  if (mood === "error") { react("error"); return; } // grumpy first, then the sad mood face
  render();
  say(mood);
}

/** Report something the mascot should react to: no_key, offline, error, empty, pomo, session, ai, streak. */
export function signal(key, value) {
  const prev = signals[key];
  signals[key] = value;
  if (key === "error" || key === "ai" || key === "offline" || key === "no_key") everLoaded = true;
  evaluate();
  const now = Date.now();
  if (key === "session" && Boolean(value) !== Boolean(prev)) {
    if (value) { sessionSince = now; longSessionSaid = false; react("sessionStart"); }
    else { if (sessionSince) react("sessionEnd"); sessionSince = 0; }
  } else if (key === "streak" && value >= 3 && value > (prev ?? 0) && !["focus", "empty", "no_key", "offline", "error"].includes(mood)) {
    react("streak");
  } else if (key === "pomo") {
    const was = pomoPrev?.phase, is = value?.phase;
    if (was && is && was !== is) lastEnded = was;
    pomoPrev = value;
  }
}

/** A short celebration (e.g. a Pomodoro phase finished): proud after a focus round, excited otherwise. */
export function cheer() {
  react(lastEnded === "focus" ? "pomoDone" : "cheer");
}

/** A focus round just ended: react to how much of it was spent coding (percent, 0-100). */
export function focusEnd(percent) {
  react(percent >= 80 ? "focusHigh" : percent >= 40 ? "focusMid" : "focusLow");
}

function greetingKind(hour = new Date().getHours()) {
  if (hour >= 5 && hour < 12) return "morning";
  if (hour >= 12 && hour < 18) return "afternoon";
  if (hour >= 18 && hour < 23) return "evening";
  return "night";
}

export function setTheme(t) {
  theme = t;
  $("mascot-bar").dataset.hasSprite = String(Boolean(t.sprite));
  render();
  if (!t.sprite) return;
  if (mood === "idle") react(greetingKind()); // switching theme = meeting a new character
  else say(mood);
}

export function currentMood() {
  return mood;
}

export function currentExpression() {
  return expression();
}

function onPoke() {
  const btn = $("mascot-btn");
  btn.classList.remove("is-poked");
  void btn.offsetWidth; // restart the hop animation
  btn.classList.add("is-poked");
  if (drowsy) { drowsy = false; react("wake"); return; }
  const now = Date.now();
  pokes = pokes.filter((t) => now - t < 4000);
  pokes.push(now);
  if (pokes.length >= 6) { pokes = []; react("dizzy"); }
  else if (pokes.length >= 3) react("pokeMore");
  else hold(Math.random() < 0.25 ? "wink" : "happy", "poke", 1200);
}

/** Small idle behaviours: an occasional wink, glance or smile, rarely with a line. */
function fidget(now) {
  nextFidget = now + 25000 + Math.random() * 35000;
  if (override || drowsy || loading || !CALM.has(mood)) return;
  const r = Math.random();
  const quiet = now - lastSaid > 90000 && $("mascot-say").classList.contains("is-faded");
  if (quiet && r < 0.3) react("chatter");
  else if (r < 0.55) hold("wink", null, 450);
  else if (r < 0.8) hold("thinking", null, 1400);
  else hold("happy", null, 900);
}

function tick() {
  const now = Date.now();
  if (!drowsy && !override && CALM.has(mood) && now - lastActive > DROWSY_AFTER) {
    drowsy = true;
    render();
    say("drowsy");
  }
  if (sessionSince && !longSessionSaid && now - sessionSince > LONG_SESSION && !override) {
    longSessionSaid = true;
    react("longSession");
  }
  if (now >= nextFidget) fidget(now);
}

export function init() {
  const btn = $("mascot-btn");
  btn.addEventListener("click", onPoke);
  // Petting: stroke the mascot with the pointer for a bit.
  let stroke = 0, lastX = null, lastY = null, lastPet = 0;
  btn.addEventListener("pointermove", (e) => {
    if (lastX !== null) stroke += Math.abs(e.clientX - lastX) + Math.abs(e.clientY - lastY);
    lastX = e.clientX; lastY = e.clientY;
    if (stroke > 240 && Date.now() - lastPet > 8000) { stroke = 0; lastPet = Date.now(); react("love"); }
  });
  btn.addEventListener("pointerleave", () => { stroke = 0; lastX = lastY = null; });

  // Any input counts as activity and wakes a dozing mascot.
  const active = () => {
    lastActive = Date.now();
    if (drowsy) { drowsy = false; react("wake"); }
  };
  for (const ev of ["pointerdown", "keydown", "wheel"]) document.addEventListener(ev, active, { passive: true });
  let lastMove = 0;
  document.addEventListener("pointermove", () => { if (Date.now() - lastMove > 1000) { lastMove = Date.now(); active(); } }, { passive: true });

  // Thinking face while stats load (first load also gets a line).
  const panel = $("panel-stats");
  if (panel) {
    let busyTimer = null;
    new MutationObserver(() => {
      const busy = panel.getAttribute("aria-busy") === "true";
      clearTimeout(busyTimer);
      if (busy) {
        busyTimer = setTimeout(() => {
          loading = true;
          if (!everLoaded && !override) say("loading");
          render();
        }, 400);
      } else if (loading) {
        loading = false;
        render();
      }
    }).observe(panel, { attributes: true, attributeFilter: ["aria-busy"] });
  }

  // Blink now and then.
  const blinkLoop = () => {
    setTimeout(() => {
      blinking = true;
      render();
      setTimeout(() => { blinking = false; render(); blinkLoop(); }, 150);
    }, 2500 + Math.random() * 3500);
  };
  blinkLoop();
  setInterval(tick, 5000);
}
