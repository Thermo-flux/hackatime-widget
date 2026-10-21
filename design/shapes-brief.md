# Theme silhouettes: brief

Goal: each theme changes the widget's outline, not only its colours. All shape work is in `ui/css/shapes.css`
(imported last). The window stays 320x436 and transparent. Content must stay fully inside every shape and stay
clickable. `tools/shape-check.mjs` enforces this by hit-testing.

## Budget (why the shapes look the way they do)
- The content is a rectangle that is nearly full width. The header (3 tabs, Demo pill, x) needs about 253px
  (Verdana/Segoe Print/mono fonts need 265-285px), and the content reaches into all four corners: tabs top-left,
  x top-right, mascot bottom-left, Theme bottom-right. So a silhouette can only bite about 25-35px into the corners
  (radius r costs about 0.3r of inset), plus whatever hangs outside the card in the `--pad-*` gutters (max 28px).
- Vertical room at 436px tall is roughly 40px on stats/sessions and 20px on pomodoro. The hidden test window is
  29px shorter. To make room without a rectangle, some shared layout gets elastic (applies to all themes, so
  classic looks the same at full size):
  pomodoro ring = clamp(108px, free height, 150px); the 14-day chart can shrink to 22px; the language list
  wraps languages that don't fit into a clipped second column, so whole rows disappear instead of being cut;
  header tabs may shrink (`min-width: 0`), shaped themes use slimmer tab/gap padding, and wide-font themes get
  a smaller tab font. (Before this work dog/ghost/mushroom/phosphor already pushed the x button out of the card
  at the hidden window's width.)
- Outline technique: themes that only use `border-radius` keep the real border + box-shadow. Themes using
  clip-path/mask, or with parts added outside the card, set the card border transparent and draw one outline
  around card + decorations with zero-blur `drop-shadow` stacks on `.shell` (cheap). Blur on `.shell` stays
  at 6px or less because the mascot animates inside it. Animated decorations live in themes without a `.shell`
  filter.
- `.picker` inherits the card's padding, so the overlay follows the same insets. On shaped themes its title is
  centred and the Done button is narrower, so neither runs into a curved corner.
- Short cards tighten their rhythm with a container query on `.card` (content box <= 385px: 8px view gaps, smaller
  tiles/footer). Classic at the real window is 390px and is untouched. Below 346px (only the hidden test window)
  the API-key form also drops the useless range tabs.

## Themes (as built)

| theme | silhouette | technique | decorations | pads t/r/b/l | content insets | risks |
|---|---|---|---|---|---|---|
| classic | plain rounded rectangle (unchanged) | - | - | 6 | 16 | - |
| avocado | avocado half: big round shoulders, tighter bottom, dark skin, light-green rim, yellow-to-green flesh | elliptical border-radius, 5px border, inset box-shadow rim, radial-gradient flesh | `deco-a` stem, `deco-b` leaf | 14/6/6/6 | 32 top, 20 sides | tabs/x vs the shoulders (padding-box radius is what clips) |
| tomato | squat round tomato with a red skin edge and a gloss streak | elliptical border-radius, 3px accent border, `.card::before` gloss | `deco-a` star calyx (clip-path inside a filtered wrapper for its outline), `deco-b` stem; both only over the top padding | 20/6/8/6 | 26 top, 22 sides, footer +4 | x button in the top-right curve |
| cat | rounded head with two pointy ears and pink inner ears | ears are clip-path triangles behind the card; one outline around card + ears via `.shell` drop-shadows | `deco-a/b` ears, `::after` inner ear | 26/8/6/8 | 16 | none found |
| dog | rounded head with long floppy brown ears hanging behind both sides, red collar with a gold tag | teardrop border-radius ears behind the card, rotated outward; collar = `.card::after` in the bottom padding | `deco-a/b` ears, `.shell::after` tag | 8/19/14/19 | 12 sides, Verdana tabs 10.5px | header width (Verdana) |
| neon | sci-fi hull: asymmetric chamfers, notches in both sides, cyan glowing edge, two antennas with blinking lights | `clip-path: polygon()` on the card; outline + 5px glow on `.shell` | `deco-a/b` antennas (`::after` lights blink with steps()) | 22/6/6/6 | 16 | buttons have their own chamfer clip (checker allows for it) |
| frog | wide body with two big eye bumps on top | bump circles behind the card, union outline | `deco-a/b` bumps, eyeball/pupil/highlight in `::before/::after` | 26/6/6/6 | 16 | none found |
| ghost | ghost sheet: domed top, round scallops along the hem | `mask`: solid body + repeating radial-gradient scallops (`round`); dome via border-radius; outline + soft glow on `.shell` | - | 6/6/6/6 | 24 top, 20 sides, 30 bottom; Segoe Print tabs 12px | header vs dome |
| mushroom | red spotted cap (header sits on it) over a narrower cream stem, grass tufts at the base | card background paints the cap, spots and gills; `clip-path: shape()` pinches the stem in under the cap rim (polygon fallback); header widened back with negative margin | `deco-a/b` grass | 8/6/6/6 | stem 26 sides, header 12 | x colour on red (made cream); `shape()` needs Chromium 135+ |
| boba | tapered cup, lid rim + dome, fat pink straw, tapioca pearls along the bottom | `clip-path: shape()` trapezoid with rounded bottom corners (polygon fallback); pearls are card background in the bottom padding | `.shell::before` lid rim, `deco-a` dome, `deco-b` straw behind the dome | 28/10/6/10 | 20 top, 26 bottom, footer +10 | straw can only be a short stub (28px gutter max) |
| phosphor | CRT monitor: beige bezel with stair-stepped pixel corners, rounded glass with vignette, chin with grille, label and LED | bezel = `deco-a` with a 4px-step `polygon()` behind the card; glass = elliptical radius + inset shadows + bevel ring | `deco-a` bezel (+ grille, "GREEN SCREEN" label), `deco-b` LED | 12/12/24/12 | 14/14/12, mono tabs 12px | header width |
| penguin | plump penguin: navy body ring around a white belly, rounder bottom, flippers, orange feet | elliptical border-radius, 7px navy border | `deco-a/b` flippers behind the card, `.shell::after` feet | 8/12/14/12 | 12 sides, 22 bottom, footer +10 | header width |
| jelly | jellyfish: bell dome, frilly hem, wavy glowing tentacles that sway | border-radius dome; hem = scalloped strip; tentacles = inline SVG background, transform-only sway (no `.shell` filter on this theme) | `deco-b` hem, `deco-a` tentacles | 6/6/28/6 | 26 top, 20 sides | animation cost (kept to one transform) |

Reduced motion: base.css already turns off all animations (antenna blink, tentacle sway).
