# MC630 PLT Studio v0.1.11

## Design and shaping
- Use Pen, Ellipse and Rectangle for Design, Cutline, or Print + Cut, with fill, stroke color and stroke width.
- Select design shapes and manual cutlines together with Shift+click; use Weld, Trim and Intersect from the Tool Options Bar.
- Resize cursors follow corner handles. Shift preserves proportions or constrains movement, Alt resizes from the center, Ctrl+drag duplicates, and Escape cancels the transform.
- Copy/Paste design shapes and manual cutlines with Ctrl+C/Ctrl+V or Edit > Copy/Paste, preserving style and Print/Cut settings.
- Save/open, SVG print export, 300 DPI native printing and PLT export preserve the new design geometry.

## Manual cutlines and layers
- Select standalone manual cutlines in Layers and link them to an image using the chain icon on its layer.
- Unlink retains the cutline on the page, including its current orientation and outline distance; other image copies retain their links.
- Link/unlink and shaping support Undo/Redo.
- Preserve sharp corners now also applies to outline offsets with smoothing switched off. Conservative polygon detection removes pixel-scale kinks while retaining curved contours.

## Preview and light UI
- Background design preview rendering keeps zoom and page movement responsive; cached previews remain visible while replacements render.
- Detail previews follow zoom and display DPI after navigation settles, with immediate cached movement/resizing for a single design shape.
- Light color picker with hue and saturation/value controls, RGB/alpha and HEX input, New/Current previews, and explicit OK/Cancel.
- Color picker input is isolated from the canvas.
- Consistent dialog headers, margins and right-aligned action buttons; compact controls, page-side switch, overflow notices and dot-settings reset.
- Drag a selection box from empty canvas space to select images; Shift/Ctrl adds to the selection.

## Current limitations
- Shaping requires closed paths. Design operations producing inner holes keep the original shapes and show a message; manual cutline operations support hole rings.
- Large raster image paste still rebuilds a PNG atlas and retraces images; this release does not claim to resolve that measured bottleneck.
- Physical printer/cutter alignment requires verification on the actual device.

Validation: 175 automated tests passed in release mode; 4 manual/device/performance tests remain ignored.

Download MC630-PLT-Studio.exe and verify it against SHA256SUMS.txt. Existing installations can use Studio updates.
