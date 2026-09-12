# Image assets

All five displayed raster assets were generated with the built-in imagegen tool, visually inspected, and copied into this repository. No external image service is needed at runtime. The supplied screenshot is the visual reference; its text was treated as sample UI content.

| File | UI use | Final generation prompt |
| --- | --- | --- |
| `public/assets/mountain-hero-v2.png` | Panoramic header | Preserve panoramic mountain scene; shrink lone hiker to about 120px tall near x1200 in a roughly 2117×743 image, add milky blue-white haze, reduce mountain contrast roughly 50%, left half almost white. No UI or text. |
| `public/assets/haruka.png` | Profile and activity avatar | Photorealistic square avatar of a smiling Japanese woman about 30, shoulder-length straight dark hair and bangs, white top, pale sage background. Front view, centered head and shoulders, circular-crop headroom, soft daylight, realistic skin texture. One person, no text, UI, logos, or watermark. |
| `public/assets/kenta.png` | Activity and member avatar | Square natural photo avatar of a Japanese man around 30, short dark hair, white shirt and navy blazer, slight smile, centered head and shoulders, circular-crop headroom, pale gray/sage background, soft daylight. One person; no text, UI, logos, or watermark. |
| `public/assets/plant-quote.png` | Learning quote background | Photorealistic compact quote-card background, wide landscape. Pale white and icy blue, delicate olive stems and leaves confined to far right edge. Left 70% empty. Soft natural daylight, subtle depth, restrained pastel colors. No text, UI, border, logo, or watermark. |
| `public/assets/pathbase-mark.png` | Brand mark and Tauri icon source | Three overlapping snowy mountain peaks: blue and teal foreground, tallest cobalt peak behind, navy outlines. Simple geometric brand mark, centered on transparent square background, no text or watermark. |

The first hero generation is preserved in `design-assets/mountain-hero-v1.png` for comparison. Native platform icons in `src-tauri/icons` were produced by the Tauri CLI from the generated brand mark.
