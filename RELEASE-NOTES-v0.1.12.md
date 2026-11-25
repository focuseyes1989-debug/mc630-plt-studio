# MC630 PLT Studio v0.1.12

## Vector artwork and cutlines
- Retain imported SVG artwork through zoom, crop, duplicate, project save/open and print export instead of relying on a fixed bitmap preview.
- Generate Auto cutlines from supported SVG fills and strokes, unioning color regions into a silhouette. Optional outline smoothing remains available, with a maximum smoothing distance of 10 mm.
- Improve raster polygon edges, corners and offset contour cleanup.

## Logo / Color Vector Trace
- Select a Main Page image and choose File > Vector Trace, or open Vector Trace in Artwork properties.
- Compare Original and Vector Preview with synchronized zoom and pan. The vector pane renders extra detail after navigation settles.
- Configure Max colors, similar-color merging, curve smoothing, corner preservation, speckle filtering, white-background removal and detail resolution.
- Trace on a background worker, then Use Vector or Export SVG with real paths and physical dimensions. Original pixels are retained and applying a trace supports Undo.
- Preserve enclosed white details when removing the outside white background; use premultiplied-alpha downsampling to reduce edge fringes.

## Drawing, selection and transforms
- Add an open Line Tool alongside Pen, Ellipse and Rectangle. Shift constrains lines to 45-degree steps; Design, Cutline and Print + Cut share stroke color and width.
- Add middle edge resize handles for Design shapes and manual cutlines.
- Drag a selection box from blank canvas to select Design shapes, standalone cutlines and Lines, including documents without an imported image. Shift/Ctrl adds to the existing selection.
- Multiple selected Design/Cutline objects share one transform box: move, eight-handle resize, Shift ratio/axis constraint, Alt center resize, rotation by degrees, quarter-turn rotation and flip. Escape cancels; a completed transform is one Undo step.
- Start shape drafts on mouse press, batch preview strokes, show fills immediately and cache meshes for common shapes. Concave fills retain the SVG preview renderer. Defer autosave snapshot hashing while drawing or transforming a shape group.

## Layers and compact UI
- Show attached Auto/Manual cutline entries beneath image layers, with access to cutline settings.
- Add image opacity from 0–100%, preserved through projects, copies and print output without changing cut geometry.
- Match the opacity slider handle to the other compact sliders.
- Remove the bottom Control bar and compact the Page size dialog with aligned inputs, the selected paper preset and an inline width/height swap button.

## Notes
- Vector Trace flattens gradients into a bounded palette and thresholds alpha at 50%. High detail is limited to 2048 pixels on the long side; tracing cannot recreate missing source detail.
- Native SVG cutlines support plain vector paths. SVG masks, filters, clip paths, patterns and embedded raster content require conversion to supported paths or manual cutlines.
- Large raster clipboard paste still rebuilds the image atlas. This release does not claim to resolve that existing bottleneck.

Validation: 206 automated tests passed; 8 manual/device/performance diagnostics remain ignored.

Download MC630-PLT-Studio.exe, verify it with SHA256SUMS.txt, and retain THIRD-PARTY-NOTICES.txt. Existing installations can use Studio updates.
