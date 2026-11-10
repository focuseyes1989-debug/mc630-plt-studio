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
