# Development

Double-click `dev.cmd` to run Studio from the current source using Cargo's debug profile.
Rust and Cargo must be installed and available on PATH.

Alternatively, run this command in the repository directory:

```powershell
cargo run --locked --bin mc630-plt-studio
```

Close the development window and run the launcher again after editing the source.
Cargo recompiles changed code automatically on each launch. This is not live reload.
Build output stays in `target/debug`; the portable `MC630-PLT-Studio.exe` is not replaced.
The development app uses the usual Studio settings and project files.

## Two-sided artwork

1. Create the cut layout on **Main · Print & Cut**. Registration marks and PLT output belong to this side.
2. Switch to **Back · Artwork**, then import the back image. The red ghost shows the main cut contours reflected across the full sheet.
3. Choose **Left / right** or **Top / bottom** to match how you turn the paper. Back artwork stays readable; the app does not mirror its pixels or text.
4. Drag back images, edit X/Y and width/height, or select a cut contour and use **Fit to cut contour**. Fit fills that contour's bounding rectangle and can change the image's proportions.
5. Print Main first at 100%. Turn and reinsert the same sheet, then print Back at 100%, with driver duplex disabled. These are two separate print jobs. Feed orientation depends on the printer.
6. Use **Back print correction X/Y** after a test sheet if the printer introduces a consistent offset. These values affect back artwork only.

Ghost contours and registration marks are excluded from back printing and back PNG export. The main PLT is unchanged by back artwork. Both sides are embedded in the `.mc630` project and included in recovery saves.

Use this development version when editing projects with back artwork; earlier releases do not preserve the new back-side data when saving. Physical front/back registration still needs a test sheet on the actual printer.

### Back artwork editing

- Select an image, then drag to move it. Shift constrains movement to one axis; Ctrl+drag makes a copy.
- Drag a corner handle to resize. Link width/height preserves the aspect ratio; turn it off for independent scaling. X/Y and width/height also accept exact values in the current unit.
- Angle accepts degrees clockwise. Use the 90-degree buttons for quarter turns, or Flip H / Flip V to mirror the image. Reset clears rotation and flips.
- Ctrl+C copies the selected image. Ctrl+V pastes it with its size, angle and flips preserved. Photoshop clipboard images can also be pasted.
- Ctrl+D or Ctrl+R duplicates the selection. Arrow keys move it by 0.1 mm; Shift+arrow uses 1 mm. Delete removes the selection.
- Rotation and flips appear in the preview, back print, PNG export and saved project. Red ghost contours remain fixed while artwork is transformed.

### Project save / open

Save and Open run file access, image preparation and serialization on a background worker. A progress dialog shows the current phase while editing is paused. Large image previews are prepared before committing the opened document; repeated back images share preview textures.

Save All waits for each file to finish. Save-and-close proceeds only after a successful write. Failed opens preserve the current document, and failed saves leave unsaved changes intact. The `.mc630` format remains compatible with existing projects.

### Retained SVG artwork and native SVG cutlines

Import the original `.svg` on Main Page to preserve its vector data in the project. At high zoom the visible region is re-rendered from the SVG on a background worker. Crop, orientation, copies, project save/open and print export preserve the vector source. Older projects containing only the converted PNG need the original SVG re-imported.

In Auto outline mode, plain SVG paths use their native filled/stroked silhouette rather than the bitmap alpha trace. Visible regions are unioned so internal color boundaries are not cut. Curves are flattened at a 0.01 mm geometric tolerance before cutter-unit rounding; Off preserves the native SVG outline; optional smoothing can clean irregularities in the original paths and offset joins without raster tracing. Outline distance and Cut inner holes remain available. Image-frame and geometric-ellipse cut modes retain their existing behavior.

SVG masks, filters, clip paths, pattern paint and embedded raster images are not silently treated as exact vector silhouettes. Convert those features to plain vector paths in the source editor or draw a manual cutline. Physical machine calibration and blade/printer alignment are separate from SVG geometry precision.


### Color / logo vector trace

On Main Page, select an image and choose File > Vector Trace, or Vector Trace in Artwork properties. Choose Logo (corner preservation) or Color Artwork, 2–32 palette colors, speckle filtering and a Fast/Standard/High detail limit (768/1536/2048 pixels on the long side). Click Trace / Update Preview to compare the transparent original with fitted vector curves. Calculation runs on a worker; changing settings requires an explicit updated preview before Use Vector or Export SVG is enabled.

Use Vector retains the original bitmap and attaches SVG artwork to that image. Existing layout, opacity and manual cutline choice are preserved; Ctrl+Z restores the previous artwork. Saved projects, duplicates, canvas vector previews and print exports retain the traced source. In Auto mode the SVG silhouette is used for the cutline; internal color boundaries are not separate cuts. Export SVG writes real paths with the selected image's physical size, not an embedded bitmap. Color traces flatten gradients and convert alpha below 50% to transparency; white removal is optional. High-detail trace is bounded to 2048 pixels to keep memory/work predictable.

The trace engine uses VTracer 0.6.5 (MIT/Apache-2.0), https://github.com/visioncortex/vtracer, with a bounded median-cut palette and output palette snapping. Upstream license notices are retained under THIRD_PARTY/vector-trace/.


Vector Trace quality controls: Max colors is an upper bound. Merge similar colors (Logo default 18, Color Artwork default 4, 0 disables merging) combines nearby palette shades using population-weighted colors, reducing noise without blurring the raster. Curve smoothing controls curve fitting independently from Preserve corners. Input downsampling uses premultiplied-alpha Lanczos filtering to prevent dark/white RGB fringes around transparency. Remove white background floods only edge-connected transparent/near-white background; enclosed white details are retained.

Preview zoom is shared by both panes (100–800% relative to Fit). Drag either pane to pan; scroll anchors zoom under the pointer; Fit resets both. The original uses the bounded source pixels, while the vector pane uses the same adaptive SVG region renderer as the canvas to keep zoomed edges sharp without rebuilding the trace.


### Line tool

The left toolbar's diagonal Line icon draws an open two-point segment by dragging. Shift constrains it to 45-degree steps; Escape cancels the active draft. Horizontal and vertical lines are supported, with endpoints clipped to the page while retaining constrained angles. Tool Options shares Design / Cutline / Print + Cut, stroke color and width with the other drawing tools. Open lines have no fill and remain open in saved projects, print SVG and PLT output. Undo, selection and shape copy/paste use the existing shape workflow.


### Responsive rectangle / ellipse drawing

Rectangle, ellipse and line drafts begin on mouse press, with one connected paint path rather than a separate draw command for every segment. Design fills appear live during the gesture. Common convex filled shapes and open strokes use an immediate vector-mesh preview cached by geometry, page transform and display density; concave and self-crossing fills keep the SVG renderer. Print/SVG/PLT data still uses the original physical paths. Cut overlays are painted once, above the artwork. Camera easing is stopped during a shape drag, and autosave snapshot hashing is deferred until release.

The ignored `drawing_cpu_frames` diagnostic records CPU-only egui frame/tessellation timings with 40 existing design shapes; set MC630_SHAPE_PROFILE to choose its JSON output. It does not measure GPU/presentation or physical input latency.


### Selection marquee for designs and cutlines

With Select active, drag from blank canvas to select images, Design shapes, standalone manual cutlines and Line objects, including documents without imported artwork. Shift/Ctrl preserves the previous selection and adds intersecting objects. Shape selection tests actual path segments and closed interiors rather than just a diagonal line's bounding box. Hidden cut-only objects are excluded when cutline display is off. Box selection highlights feed the existing shape selection / copy / shaping workflow.


### Transform selected Design / Cutline objects together

Two or more selected Design shapes or standalone cutlines share one bounding box with eight resize handles. Drag the box to move the set; drag a corner or edge handle to resize all paths around one shared anchor. Shift locks the resize ratio (or movement axis), Alt resizes about the shared center, and Escape restores the gesture's starting geometry. Clicks below the drag threshold do not edit geometry. A completed gesture creates one Undo step for the mixed selection, preserving each shape's fill, stroke and Print/Cut flags. Tool Options provides shared Rotate 90, Flip H/V and Rotate by degrees + Apply controls. The selected objects remain separate layers.
