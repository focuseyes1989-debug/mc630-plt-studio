# MC630 PLT Studio v0.1.10

## UI and project navigation
- Light-theme Recent Projects gallery with six compact cards in one row, grid/list views, sorting, filtering, and per-project removal from history.
- Recent-only scrolling and stable card sizing after maximize/restore.
- Linked page margins with edge icons, icon-only portrait/landscape controls, width/height linking, spacing sliders, and Fill Sheet beside Copies.
- Disable document-only toolbar actions before opening a project; remove redundant home import area and Quick Guide toolbar shortcut.
- Open saved projects centered at Fit Page instead of restoring an off-screen zoom/pan.

## Pen and manual outlines
- Windows-native nib cursor that moves independently of canvas redraws.
- Insert anchors between existing Pen points and Shift-drag anchors to bend curves without altering the curve when inserting a point.
- New image imports start in Manual mode, with an explicit Manual/Auto choice.
- Link manual outlines to images so move, resize, rotation, flip, duplication, copy/paste, and project save/open preserve the outline.
- Manual outline distance, smoothing, sharp-corner preservation, and consistent preview/PLT export.
- Repeat the source image outline on all Copies / Fill Sheet instances, including cropped images.
- Studio Copy/Paste preserves linked outlines, crop and orientation.

Validation: 150 automated tests passed; 3 manual/device tests remain ignored. Physical printer/cutter alignment has not been verified by these automated tests.

Download MC630-PLT-Studio.exe. Verify it against SHA256SUMS.txt.
