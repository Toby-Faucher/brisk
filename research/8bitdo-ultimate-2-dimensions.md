# 8BitDo Ultimate 2 Wireless: dimensions and layout for an SVG outline

Date 2026-10-25. Research only. [U] = not verified. Goal: enough numbers to draw an original outline (no photos copied or traced; nothing embedded). Toby's unit: Chrome '8BitDo Ultimate 2 Wireless (STANDARD GAMEPAD, 2dc8:6012)' / 2.4G receiver `310b`. Model numbers seen at retailers: 81HE02 black, 81HE01 white [U, retailer listing].

## TL;DR

| item | value | source |
|---|---|---|
| Overall size (W x D x T) | 147 x 103 x 61.3 mm | [S1] official spec "147*103*61.3 mm" |
| Which axis is which | W 147 = across grips, D 103 = top (bumpers) to grip tips, T 61.3 = front-to-back incl. sticks/triggers | [U] inferred; the manual's front diagram measures 137 x 98 pt = ratio 1.40 vs 147/103 = 1.43, consistent |
| Weight | 246 g | [S1] |
| Layout | Xbox style, asymmetric: left stick upper-left, d-pad lower-left, face buttons upper-right, right stick lower-right | [S1] "Xbox layout", [S2] diagram |
| Sticks | TMR (not Hall), RGB "Fire Ring" around each, 12-bit ADC | [S1], [S2] |
| Triggers | Hall effect, switchable (slider per trigger) to non-linear tactile (digital click) | [S1], [S2] |
| D-pad | tactile; plus-shaped (cross) with round centre dish | [S1], [S2] |
| Extra inputs | L4/R4 "fast bumpers" on top edge by the USB-C, PL/PR back paddles, Profile button, Star button, Square (Pro/mapping) button; 3 custom profiles | [S1], [S2] |
| USB-C | top edge, centre, between the bumpers | [S2] |
| Mode switch | back, centre; slider with two positions, BT and 2.4G | [S2] |
| 2.4G receiver | sits in the dock; dock is a separate base, not part of the outline | [S1], [S2] |
| Stick cap, button diameters, spacings | not published; scaled from the manual diagram (below) | [S2] scaled, approximate |

## Sources

- [S1] Official product page, fetched 2026-10-25: https://www.8bitdo.com/ultimate-2-wireless-controller/ (spec table: 147*103*61.3 mm, 246 g, TMR sticks, Hall effect triggers switchable to tactile, L4/R4 fast bumpers, 2 Pro back paddles, Xbox layout, 3 profiles, dock auto-reconnect).
- [S2] Official manual (EN panel, page 1 button diagram): https://download.8bitdo.com/Manual/Controller/Ultimate/Ultimate-2-Wireless-Controller-EN.pdf (linked from support.8bitdo.com). A single huge vector page with all languages side by side; no text layer, so I rendered it locally and measured the vector line art. Diagram is a stylised line drawing, not a dimensioned drawing.
- Retailer cross-check (secondary, [U]): Digitec/Galaxus list 246 g and 14.7 x 6.1 x 6.13 cm; Euronics 14.7 x 10.3 x 6.13 cm. Agrees with [S1] on weight and on 147 / 103 / 61.3.
- FCC filing: not found. fccid.io returned HTTP 403 and search did not surface the FCC ID. [U] No FCC photo or dimension data collected. Try the FCC OET equipment search for model 81HE02 by hand.
- Not found: any official dimensioned drawing, stick/button diameters, or button pitch.

## Controls (from the manual diagram)

Front (top view, held normally): Home button (8BitDo heart/star logo, with Status LED ring) top centre; View (minus) left of it, Menu (plus) right of it. Row below: Square button (left), Mapping LED (tiny dot, centre), Star button (right). Profile button (small horizontal pill) centre, with 3 Profile LEDs stacked vertically below it. Face buttons diamond: Y top, X left, B right, A bottom. Left stick with ring at upper left; d-pad at lower left; right stick with ring lower right, centre-right.

Back view: L4 and R4 at the top corners next to the USB-C; two small trigger-mode slide switches just below; PR and PL paddles inside the grips (left grip holds PL, right grip PR, as drawn: PR on the left of the picture because it is a back view); mode switch (BT / 2.4G slider) and 3 dots at centre.

Top edge: the manual's third view looks at the top edge from beyond it (from the far side, facing the player), face side up, so it is mirrored left-right: RB/RT appear on the left of that picture and LB/LT on the right. On the controller in the player's hands, LB, LT and L4 are on the player's left and RB, RT and R4 on the right, as on any Xbox-layout pad. In that view L4/R4 sit inboard of the triggers (towards the USB-C), and between the two sides are the power LED, USB-C and pair button. [U] exact order of power LED / USB-C / pair on the physical unit.

Re-measure (2026-10-25, for web/src/modules/settings/pad.svg.ts): the manual's front diagram is vector art; its body box is 137.19 x 96.24 pt, ratio 1.4255 vs 147/103 = 1.427, so at 1.0715 mm/pt it is 147 x 103.1 mm. Positions taken straight from the vector shapes (centre of body box as origin) differ from the table below by up to about 0.7 mm (e.g. left stick y -22.8, face buttons 9.7 mm, home y -35.3); pad.svg.ts uses the re-measured values.

## Chrome standard mapping

Chrome exposes 17 standard buttons: A B X Y LB RB LT RT Back(View) Start(Menu) L3 R3 D-pad x4 Home. Extras (L4/R4, PL/PR, Star, Square, Profile) are not in the standard mapping. Per the manual they can be mapped onto an existing input (single or multiple buttons mapped to L4/R4/PL/PR via hold + button combos), and Ultimate Software does the rest. [U] I did not test whether any extra shows as button index 17+ on the 2dc8:6012 device.

## Drawing guide

Scale and frame: 1 SVG unit = 1 mm. Suggested viewBox `-80 -60 160 120` (body about 147 x 104, margin). Origin = centre of the body bounding box (x from the grip outer edges, y from the bumper tops to the grip tips). y is down. All positions below were scaled from the manual's front diagram (diagram 137.3 pt wide -> 147 mm, factor 1.071 mm/pt) and are accurate to roughly +-1.5 mm [U].

Controls, centre x,y (mm) and diameter (mm):

| control | x | y | diameter |
|---|---|---|---|
| Left stick cap | -42.4 | -22.1 | 15 |
| Left stick Fire Ring (outer) | -42.4 | -22.1 | 24 |
| Right stick cap | 20.3 | 1.3 | 15 |
| Right stick Fire Ring (outer) | 20.3 | 1.3 | 24 |
| D-pad (cross span; arm about 8.5 wide; centre dish d about 6) | -20.3 | 1.3 | 26 span |
| Y | 42.5 | -32.7 | 10 |
| X | 31.9 | -22.1 | 10 |
| B | 53.0 | -22.1 | 10 |
| A | 42.5 | -11.6 | 10 |
| Home (logo) | 0 | -34.7 | 10 |
| View (minus) | -19.0 | -34.7 | 8 |
| Menu (plus) | 19.8 | -34.7 | 8 |
| Square button | -9.7 | -23.4 | 8 |
| Star button | 10.1 | -23.4 | 8 |
| Mapping LED | 0 | -23.4 | 2 |
| Profile button (pill) | 0 | -11.3 | 7.2 wide x 4 high |
| Profile LEDs (3 dots) | 0 | -1.3, 1.3, 4.1 | 2 |

Face-button diamond pitch is 10.5 mm from its centre (42.5, -22.1) to each button. Stick centres are 62.7 mm apart horizontally. These diameters are diagram-scaled; real caps are likely a bit larger [U].

Outline (front view, left half; mirror on x for the right). Points x,y in mm, approximate:

1. Top edge centre: (0, -49), nearly flat and slightly convex between the bumpers, running to (-26, -49).
2. Left bumper bump: rises from (-26, -49) to a peak about (-40, -52), then slopes down and outward to the tip about (-59, -43). Bump is about 3 mm proud of the top edge; shape is a low rounded wedge.
3. Shoulder: from the bumper tip curve down and out to about (-63, -36).
4. Side: bulges outward to the widest point about x = -73.5 near y = +19 (about 20 mm above the grip tips), a long gentle convex curve.
5. Grip horn: the side then curves inward and down to a rounded grip tip about (-57, +52) (bottom of the shape, y = +52).
6. Inner grip curve: from the tip, concave curve sweeping up and inward to about (-36, +22).
7. Bottom centre: a shallow arch/flat at about y = +20 spanning x = -30..+30 between the grips.

Back-view cues (not for the front outline): L4/R4 are chunky rounded blocks at the top corners; the trigger lock/mode sliders sit about 12 mm below the top edge, centred about x = +-25 [U]; PL/PR paddles are teardrop shapes inside the grips; thickness 61.3 mm is the front-to-back dimension, so a top-down thickness cue is only needed for side views.

## Unverified or missing

- FCC filing, FCC dimensions or photos: not found (fccid.io 403).
- Official stick cap diameter, button diameters, pitches, bumper size, grip radii: not published; the numbers above are measured off a stylised diagram.
- Axis assignment of 147 / 103 / 61.3 (inferred, consistent with the diagram).
- Exact top-edge component order (power LED, USB-C, pair button).
- Whether L4/R4/PL/PR appear as extra Gamepad API buttons on 2dc8:6012 (not tested).
- Dock dimensions and dock battery: not published.
- Model numbers 81HE01/81HE02: retailer listings only.
