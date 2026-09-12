# PathBase design QA

final result: passed

## Target and evidence

- Source: `/Users/takanorifukuyama/git/basepath/qa/reference.png` (user-supplied screenshot, 1672 × 941 px).
- Initial implementation: `/Users/takanorifukuyama/git/basepath/qa/desktop-before.png`.
- Final implementation: `/Users/takanorifukuyama/git/basepath/qa/desktop-final.png`.
- Local browser: `http://localhost:1420/`, Codex in-app browser.
- Matched viewport: 1672 × 941 CSS px, devicePixelRatio 1. Final source and implementation images are both 1672 × 941 px; no density scaling was required.
- Matched state: home, all scopes, regional-event goal selected, timeline tab, initial sample data, no open dialogs, scroll position 0.
- Responsive evidence: `qa/mobile-final.png`, 390 × 844 viewport. Document width was measured as 390 px with no horizontal page overflow.
- Native evidence: `qa/tauri-native.png`, built macOS Tauri app. This is additional native smoke-test evidence; native window chrome and capture scaling are not used for pixel comparisons with the source.

## Comparison history

1. Initial full-view source and browser images were opened together in one comparison input. [P1] Map remained at the earlier viewport's scale, leaving most of its panel empty. Fixed with a container ResizeObserver and fitView on size/filter/fullscreen changes. [P2] Header hiker was too large and mountain contrast too strong. Regenerated the asset with a smaller hiker and more atmospheric haze. [P2] Small detail/task text and the related-initiative label required spacing adjustments.
2. Captured the revised interface at the same dimensions. Map now fills its intended region, with all three goals and six initiatives visible. Increased task text and separated the long related-initiative label from its buttons. Replaced the interim brand/portrait with generated final assets.
3. Real browser interaction testing found [P1] map buttons were blocked by the canvas pointer handling. Added explicit pointer events and React Flow's nodrag/nopan classes. Retested goal selection: the English goal becomes pressed, detail changes to the English title and 45% progress. Initiative modal and its 60% → 65% update were also verified.
4. Final source and rendered image were opened together again. [P2] Detail content was pushed downward and memo lines were too close to the text-area edge. Adjusted desktop detail line height, font size, and spacing. Final memo clientHeight and scrollHeight are both 56 px, so the two-line sample is fully visible. Captured and compared again; no actionable P0/P1/P2 issues remain.

## Required fidelity surfaces

- **Typography:** local Noto Sans JP for interface text; Zen Kurenaido for the handwritten header note. Main heading, sidebar, section headings, pills, and compact supporting text retain the source hierarchy. Text remains selectable and editable where applicable. Some library glyph and font-rendering differences remain as P3.
- **Spacing/layout:** map measured x256/y279, 958.375 × 371; bottom panel x256/y657, 958.375 × 272; detail x1221.375/y279, 435.625 × 650. These match the source's principal regions within approximately 1–2 px. Sidebar is 241 px wide. Rounded white panels, faint dividers, and horizontal template strip are preserved.
- **Colors/tokens:** navy text, very light cool background, pastel blue/purple/green scope colors, orange/pink template accents, and purple selected-goal/progress treatment are present. No dominant palette drift remains.
- **Images:** generated mountain header, two portraits, plant quote background, and transparent mountain mark are all bundled locally. Hero subject scale and crop were corrected after comparison. Image identity and terrain are intentionally recreated rather than pixel-identical. Phosphor supplies UI glyphs; React Flow supplies the diagram paths.
- **Copy:** Japanese heading, template names, all three goals, six initiatives, timeline, notes, learning text, and sample profile content follow the supplied reference. Dates deliberately remain the reference's April 2025 sample.

Full-view 1:1 comparisons were supplemented with DOM measurement of map/detail bounds and memo clipping, and direct inspection of the readable map and right-side details in the paired source/rendered images. Separate rescaled crops were unnecessary because these labels were legible in the equal-density full-size captures.

## Functional verification

- Browser goal selection changes the right detail panel and selected state.
- Team filter shows the team branch; all scopes restores the complete map.
- Template form creates “毎週、新しい本を読む”; the new node and detail content are visible.
- Task checkbox state changes; adding “学習ノートを整理する” adds its checkbox to today's list.
- Search for “地域” returns the regional-event goal and selects it.
- Initiative progress slider updates from 60% to 65% and the corresponding map node reflects 65%.
- Next-quarter control shows July–September; current-quarter control restores April–June.
- Reflection input is retained when switching away and back; saved state is shown.
- At 390 px, the navigation drawer opens and selecting 今日の行動 closes it and selects the corresponding tab (verified through fresh accessibility state after the drawer transition).
- Browser console: no runtime errors. One React Flow warning occurred during hot-module replacement; the nodeTypes object is defined at module scope. Library attribution is visible.
- Native macOS app launches from its `.app` bundle with images/fonts displayed. Native goal selection changes to the English goal and 45% progress.
- `npm run check`, `npm run build`, `cargo check`, and `npm run tauri -- build --debug --bundles app` passed.

## Follow-up polish and boundaries

- P3: generated photography, handwritten type, and exact diagram curves differ slightly from the reference.
- P3: React Flow attribution is retained in the lower-right corner of the map.
- The narrow mobile map is an overview; zoom/fullscreen controls are available for inspection. The supplied design is a desktop target.
- This is a UI implementation with in-memory sample data. Authentication, persistence, synchronization, and real notification delivery are outside this implementation.
- Release signing/notarization and Windows/Linux execution were not tested. The verified bundle is a local macOS debug build.

## Implementation checklist

- [x] Match major composition and visual hierarchy
- [x] Supply all image assets and local fonts
- [x] Fix interaction and clipping issues discovered in testing
- [x] Verify desktop and narrow-window behavior
- [x] Verify actual Tauri compilation, bundle launch, and goal selection
- [x] Leave browser preview available and document launch commands
