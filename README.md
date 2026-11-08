# MC630 PLT Studio

## Updates

Choose **Help → Check for Updates** to compare the installed version with the latest stable GitHub Release. If a newer Windows build is available, **Update now** downloads it inside Studio, verifies its SHA-256 checksum and restarts the app automatically. Unsaved project tabs use the normal Save / Don't Save / Cancel prompts; Cancel postpones installation. A hidden helper waits for Studio to exit, replaces the executable at the same path, and reopens the saved project tabs. Replacement or process-launch failures restore the previous executable. The app folder must be writable; no administrator prompt is used. Existing versions without this feature need one manual upgrade. Downloads are limited to stable releases from this repository; release assets must include a GitHub SHA-256 digest or `SHA256SUMS.txt`.

To publish a Windows release, update the package version in `Cargo.toml` and refresh `Cargo.lock`, commit the changes, then push a matching tag such as `v0.1.1`. The Windows release workflow tests the tagged source and attaches the executable and its SHA-256 checksum to GitHub Releases. The first release can use `v0.1.0` while the package version is `0.1.0`.

## CCD 4-dot trial (experimental)

App မှာ **CCD 4-dot trial (EXPERIMENTAL)** ကို ရွေးနိုင်ပါတယ်။ ထုတ်လုပ်သူ MC630 CCD ဗီဒီယိုမှာတွေ့တဲ့ black circle လေးခုကို အခြေခံထားပါတယ်။ **MC630 Ver5.022 နဲ့ scan အောင်မြင်ကြောင်း မစမ်းရသေးပါ။** Dot diameter 5.5 mm နဲ့ inset 7 mm က စမ်းသပ်တန်ဖိုးတွေဖြစ်ပြီး firmware specification မဟုတ်ပါ။ Mark Position 5.5 mm ဆိုတဲ့ video setting ကို dot diameter လို့ မအတည်ပြုနိုင်ပါ။

ဒီ mode က `PRINT.html` နဲ့ `CCD_TEST.plt` ထုတ်ပါတယ်။ Manual offsets ကို မသုံးဘဲ mark အောက်ဘယ်နေရာကို relative origin ယူပြီး TB25 ကို mark center spans နဲ့ တွက်ထားပါတယ်။ First dot / feed orientation / TB25 axes ကို ဒီ firmware မှာ မအတည်ပြုရသေးပါ။ **ဓားဖြုတ်ပြီးမှ scan စမ်းပါ**—ဖိုင်မှာ contour တွေပါလို့ scan အောင်ရင် စက်ဆက်လှုပ်ရှားနိုင်ပါတယ်။ ယခင် `+` ပါ print စာရွက်ကို ဒီဖိုင်နဲ့ မသုံးပါနဲ့။

[သုတေသနအထောက်အထားနဲ့ ကန့်သတ်ချက်များ](research/CCD-FINDINGS.md) ကို ကြည့်ပါ။ `examples/CCD-TRIAL/` မှာ နမူနာ ၂၀ ပုံ စမ်းသပ်ဖိုင်ရှိပါတယ်။ Manual mode က default အဖြစ် ဆက်ရှိပါတယ်။

## A4 Print + Outline Cut (new default screen)

### Direct Windows printing

App ထဲက **Printer** မှာ printer ရွေးပြီး **Printing Preferences...** ကိုနှိပ်ပါ။ Printer driver ရဲ့ paper / media / quality / color / copies setting တွေကို ရွေးပြီး OK နှိပ်ပါ။ A4 Portrait၊ scale 100% သုံးပါ။ ထို့နောက် **Print directly to selected printer** နှိပ်ရင် လက်ရှိ A4 layout ကို Windows print queue ဆီ တိုက်ရိုက်ပို့ပါတယ်။ Browser ဖွင့်ဖို့ မလိုပါ။

ရွေးထားတဲ့ printer နဲ့ driver preferences ကို App အတွက် သိမ်းထားပြီး နောက်တစ်ခါ ပြန်သုံးပါတယ်။ Windows default preferences ကို မပြောင်းပါ။ A4 Portrait မဟုတ်ရင် print ကို ရပ်ပြီး preferences ပြင်ဖို့ ပြပါတယ်။ Driver ရဲ့ Fit / borderless expansion ကို ပိတ်ထားပြီး 20 mm ruler ကို တိုင်းစစ်ပါ။ Cut lines မပါဘဲ images နဲ့ လက်ရှိ registration marks ကို 300 dpi နဲ့ print ထုတ်ပါတယ်။ Queue ဆီ ပို့ပြီးကြောင်းသာ App က အတည်ပြုနိုင်ပါတယ်။ စာရွက်တကယ်ထွက်ပြီးမှုကို printer queue မှာ စစ်ပါ။ PLT ကို **Export PRINT + CUT pair** နဲ့ ဆက်ထုတ်နိုင်ပါတယ်။

တစ်ပုံတည်းကို A4 တစ်ရွက်မှာ အရေအတွက်အလိုက် စီပြီး ပုံအပြင်ဘက် outline အတိုင်း ဖြတ်ရန်:

1. **Choose image** နဲ့ transparent PNG ဖွင့်ပါ။ JPG / နောက်ခံအဖြူ PNG ဆို **Remove white background** ရွေးနိုင်ပါတယ်။ အဲဒီ option က ပုံအတွင်းအဖြူရောင်တွေကိုပါ transparent လုပ်နိုင်ပါတယ်။ Photos / နောက်ခံရှုပ်တဲ့ပုံအတွက် နောက်ခံဖယ်ပြီးသား PNG သုံးပါ။
2. Width / Height / Copies / Gap / Margin သတ်မှတ်ပါ။ **Fill A4 to capacity** နဲ့ ဆံ့သလောက် စီနိုင်ပါတယ်။ အစိမ်းရောင်လိုင်းက ဖြတ်မယ့် outline ပါ။ ပုံထဲက အတွင်းအပေါက်တွေကို မဖြတ်ပါ။ White border / bleed မထည့်ပါ။
3. **Export PRINT + CUT pair** နှိပ်ပြီး folder ရွေးပါ။ Timestamp ပါ job folder အသစ်မှာ `PRINT.html`, `CUT.plt`, `SETUP.txt` ကို တွဲထုတ်ပါတယ်။
4. **Open last print sheet** နှိပ်ပြီး browser မှာ **Print A4** နှိပ်ပါ။ A4 portrait၊ Actual size / 100%၊ Margins None၊ Headers/footers Off သုံးပါ။ Fit to page မသုံးပါနဲ့။ Print preview က တစ်မျက်နှာတည်းဖြစ်ကြောင်းနဲ့ 20 mm ruler ကို တိုင်းစစ်ပါ။ Browser ကနေ PDF အဖြစ် သိမ်းလည်းရပါတယ်။ အစိမ်းရောင် cut line ကို print မှာ မထည့်ထားပါ။
5. `CUT.plt` ကို USB stick ထဲထည့်ပါ။ စက် origin ကို အောက်ဘယ်ဘက် printed `+` အလယ်မှာ လက်ဖြင့်ချိန်ပါ။ App အသစ်ရဲ့ default က X/Y offset −7 mm စီနဲ့ **Preview origin at lower-left +** ရွေးထားပါတယ်။ အစိမ်းရောင် cut line ကို origin ရဲ့ စာရွက်ပေါ်နေရာပါ ပေါင်းတွက်ပြီး image နဲ့ ထပ်ပြပါတယ်။ Preview origin checkbox က display reference ကိုပဲ ပြောင်းပြီး PLT coordinates ကို မပြောင်းပါ။ စာရွက်ထောင့်ကို origin ယူချင်ရင် checkbox ဖြုတ်ပြီး offset 0/0 ထားပါ။ စာရွက်မစောင်းအောင်ထားပြီး pen test နဲ့ အရင်စစ်ပါ။

**အရေးကြီးတဲ့ လက်တွေ့ကန့်သတ်ချက်:** ပုံနှိပ်နေရာနဲ့ cut path တွေက software ထဲမှာ နေရာတူပေမယ့် စက်က printed image ကို အလိုအလျောက်ရှာပြီး align လုပ်နိုင်ကြောင်း မအတည်ပြုရသေးပါ။ Printed crosses က manual reference သာ ဖြစ်ပြီး MC630 camera/laser registration format မဟုတ်ပါ။ Sheet skew/rotation သို့မဟုတ် printer scale ကို အလိုအလျောက် မပြင်ပါ။ ဓားမသုံးခင် pen နဲ့ outline ကို စမ်းဆွဲပြီး တကယ်ကိုက်ညီကြောင်း စစ်ပါ။ Layout ပြောင်းတိုင်း print/cut နှစ်ဖိုင်လုံး ပြန်ထုတ်ပါ။

`examples/A4-DEMO/` မှာ synthetic transparent PNG နဲ့ 20-copy print/cut sample ရှိပါတယ်။ `cargo run --example a4_demo` နဲ့ ပြန်ထုတ်နိုင်ပါတယ်။

## Original vector tools

Rust desktop app for preparing PLT files for a USB stick. It does not send commands to the cutter.

## အသုံးပြုနည်း

1. `MC630-PLT-Studio.exe` ကို ဖွင့်ပါ။
2. **Open SVG / PLT** ကိုနှိပ်ပြီး design ဖိုင်ရွေးပါ။ SVG ထဲက စာလုံးတွေကို မူရင်း design software မှာ Convert to Curves / Object to Path လုပ်ထားပါ။ JPG/PNG ကို တိုက်ရိုက် မထည့်နိုင်ပါ။
3. Width / Length ကို မီလီမီတာနဲ့ သတ်မှတ်ပါ။ Keep proportions က ပုံအချိုးကို ထိန်းထားပေးပါတယ်။ Mirror / Rotate လည်း သုံးနိုင်ပါတယ်။ Preview က ဖြတ်မယ့်လမ်းကြောင်းကို ပြပါတယ်။
4. **Export PLT** နဲ့ USB stick ထဲ သိမ်းပါ။ စက်မှာ ဖိုင်ရွေးပြီး လက်ရှိအသုံးပြုနေတဲ့နည်းအတိုင်း ဖွင့်ပါ။ ဖိုင်နာမည်ကို `CUT001.plt` လို အင်္ဂလိပ်အတို သုံးနိုင်ပါတယ်။
5. ပထမဆုံး **New 10 mm test square** ထုတ်ပြီး ဓားဖြုတ်ထားကာ စမ်းပါ။ Pen တပ်ပြီး တိုင်းတာနိုင်ပါတယ်။

## Calibration

40 units/mm is a starting assumption, not a verified specification for this firmware. If a requested 10 mm square measures 16 mm, change units/mm to `40 × 10 / 16 = 25`. More generally: **new units = current units × requested size / measured size**. Set this before importing an existing PLT. Settings currently reset when the app closes; record the calibrated value.

The output template follows the supplied working file:

```text
TB25,<width in units>,<length in units>;CT1;IN;PA;PU0,0;
PUx,y;PDx,y;...
PU0,0;!PG;
```

`TB25` and `CT1` are copied from the working file as compatibility constants. Their firmware-specific meaning has not been verified. The original working file uses `TB25,10092,7593`; the app computes the last two values from output bounds. A successful cut of an app-exported file still needs verification on the physical machine.

## Supported design scope

- SVG vector paths, rectangles, circles, ellipses, polygons and transformed groups. Curves are flattened adaptively.
- Outlines/centerlines only: no bitmap tracing, font editing, stroke expansion, raster engraving, print-and-cut registration or automatic nesting.
- Text and bitmap elements are rejected. Clipping and masks are rejected where detected on rendered groups. Filters, fills, opacity and stroke thickness do not define additional cut geometry; prepare plain paths and check the preview.
- PLT importer supports the supplied file's absolute PU/PD coordinate style. Unsupported commands are rejected. Re-export normalizes the drawing origin and removes consecutive duplicate points; it is not a byte-for-byte copy.
- Width limit 630 mm and length limit 10000 mm are app limits, not a guarantee of the machine's usable travel. Preview Y points upward; SVG's downward Y is converted automatically.

## Build / verify

```powershell
cargo test
cargo build --release
```

Source: `src/lib.rs` (geometry, import/export), `src/main.rs` (desktop interface). `tests/reference.rs` contains a synthetic PLT round-trip fixture. `examples/test-design.svg` is a vector sample. Cargo.lock pins dependencies.

Command-line conversion is also available (refuses to overwrite existing output):

```powershell
.\plt-convert.exe .\examples\test-design.svg .\NEWCUT.plt 58 40
```

Arguments: input, output, target width in millimetres, optional units/mm. Height follows the original aspect ratio.

CCD dots now surround the occupied image frames, rather than the A4 corners. Dot center gap is measured outside the image frames. Preview, native print, HTML and PLT use the same lower-left dot origin and actual dot spans. Reprint the matching sheet after this update.

CCD origin update: top-left image-area dot is (0,0); X increases right and Y increases down. Preview and exported CCD paths share this convention. Firmware axis compatibility remains unverified. The print dots are unchanged from CCD-IMAGE-CORNERS; only cutter coordinates and preview origin changed.

Confirmed user scan success with feed-first TB25 spans (201 mm,163 mm for demo). This is now the CCD export default. Force is adjusted on the cutter panel; the app emits no explicit force command. Existing IN/CT1 compatibility initialization is retained unchanged from the successful trial. Whether initialization resets a machine setting is firmware-dependent and has not been measured.

Physical 4-column x 5-row print produced 5-column x 4-row cut and transposed asymmetric shapes. CCD export now transposes each PU/PD point to feed Y, carriage X; scan TB25 stays feed-first. Page preview and print remain unchanged. New output: examples/CCD-AXES-FIXED/CUT.plt. Hardware alignment pending user test. Copy this CCD file directly; do not re-import/re-export via SVG/PLT tools because that normalizes origin and regenerates bounds.

UI refresh: three-panel workspace with artwork/layout cards, page preview and contour toggle, production summary, native printer preferences, Print and Export actions. Advanced registration options collapse by mode. Load sample artwork previews the 20-copy layout. Print/cut geometry and machine force behavior are unchanged.

Panel-settings export: all exporters now omit IN (initialize), DF (defaults), FS (force) and VS (speed). TB25/CT1 scan/profile compatibility, PA absolute coordinates, PU/PD paths and !PG job end remain. MC630 firmware-specific TB25/CT1/end-job side effects are unverified; this change removes explicit resetting/force/speed instructions, not a guarantee of firmware state. Re-export old jobs to use this policy.

Cut Border / Outline distance: signed physical millimetres (-5 to +5). Zero keeps the original contour; positive expands outside; negative shrinks inside, with rounded joins. Applies to A4 manual and CCD paths and their preview, not printed artwork size or registration mark positions. Positive borders need sufficient Spacing and CCD dot clearance. Errors block export for collapsed outlines, likely overlapping spacing cells or CCD mark interference. Negative offsets may remove small details/components. Settings are written to each exported SETUP.txt. Use Export Print + Cut files after adjusting. Geometry uses geo polygon buffering: https://docs.rs/geo/0.33.1/geo/algorithm/buffer/trait.Buffer.html

Cut inner holes (Cut Border panel): off by default for outer-only cutting. Enable to include enclosed transparent areas in preview and manual/CCD exports; inner paths are emitted before the outside boundary. White filled pixels are not holes unless background removal makes them transparent. Tiny regions below the trace area filter are ignored. Cut Border offsets are applied to material geometry: positive offsets expand outer contours and shrink holes; negative offsets do the reverse. Offset buffering can remove small holes/features. Print raster and registration dots are unchanged. The selection is included in exported SETUP.txt.

Artwork cut shape selector: Image / transparent outline preserves the existing alpha tracing; Original image frame cuts the full rectangular source bounds; Object / plain background estimates corner background colours, interpolates gradients and removes connected matching edge pixels. BG tolerance controls this heuristic (default20). This is local colour segmentation, not semantic AI or guaranteed person/hair detection. Foreground touching corners, similar skin/background colours, shadows and busy scenes can give incomplete or damaged masks. Inspect preview, reduce tolerance to protect the object, or prepare a transparent PNG externally. Print without background toggles masked printing independently; the object cut contour uses the mask either way. Original source bytes are retained so switching modes restores the image. Frame mode ignores white removal. Cut shape, tolerance and print background choice appear in SETUP.txt.

Drawing-editor workspace refresh: File/View/Layout/Cut menus, contextual size/copies/border property bar, icon toolbox, millimetre rulers, optional10mm grid, zoom/scroll/Fit page, document tab and right Properties versus Print/Export docker. Right colour palette changes preview contour colour only. Zoom and screen grid/rulers/palette are not part of print or PLT output. Toolbox icons: Import, Properties, Print/Export, Cut paths, Grid, Fit page. No geometry or printer settings changed.

Per-image canvas editing: click a repeated image to select its frame, drag the image to move it, or drag one of its four square corner handles to resize. Lock ratio / Keep proportions preserves aspect ratio; unlock for independent width and height. The top property bar shows selected-image X/Y/W/H in physical mm. Click blank page or Esc to deselect. Edit menu / Ctrl+Z and Ctrl+Y undo or redo object edits. Layout > Reflow all copies to grid clears manual placements (undoable). Repeat size/spacing defaults affect unedited copies; manually edited copies retain their own positions/sizes. Overlapping manual placements are permitted; check the sheet visually. Placement edits stay inside A4, with extra room for CCD dots when CCD is enabled. CCD marks automatically surround the actual per-image frame extents. Reprint and re-export after edits because dot bounds may move. Print HTML, native raster, preview and both manual/CCD PLTs use the same per-image placements. Exported LAYOUT.csv records every image position and size. Edits are held in the current app session; export before closing.

File > New document (Ctrl+N): A4, A3, A5, A6, A2, A1, Letter, Legal, Tabloid, ISO B4/B5 or Custom. Portrait/Landscape swaps dimensions. MC630 page width is limited to50-630mm; height50-2000mm. Create document clears current artwork/manual placements/history and keeps printer preferences. New pages adapt repeat capacity. Paper dimensions drive page/rulers, manual move/resize bounds, physical cut coordinates, CCD geometry, HTML @page/SVG, and300dpi native raster. Printer paper size and orientation must match the document; mismatches stop before spooling. Driver custom-paper support is printer-dependent. Direct raster printing is limited to80million pixels to bound memory. A3Landscape420x297mm was smoke-tested with Microsoft XPS Document Writer; no physical printer job was sent.

### Manual outline smoothing
Properties > Cut Border offers Outline smoothing (0-2 mm), with Off, Light (0.2 mm), and Medium (0.5 mm) presets. Zero preserves the original contour exactly. Increasing the value rounds corners and small details using uniform arc-length resampling and a periodic Gaussian filter in physical millimetres, after Cut Border is applied. Both outer contours and enabled inner holes use the same smoothing in preview and PLT, including individually resized copies. Printed artwork, CCD dots, machine force and speed are unchanged. Start at 0.2 mm and inspect the preview before exporting a fresh PLT; fine details may be softened.

### Canvas navigation
Hold Space and drag with the left mouse button to pan the page without moving artwork. Ctrl + mouse wheel over the canvas zooms from 50% to 1600% around the pointer. Normal dragging still moves/resizes individual copies. Fit page resets both zoom and pan. Navigation affects only the view, never print/cut geometry.

Outline preview now caches geometry until artwork or cut/layout settings change; panning and zooming reuse it. CCD reference geometry is computed once per frame rather than per vertex. Smoothed contours discard redundant segments at a 0.005 mm simplification tolerance, shared by preview and PLT.

Smoothing now acts over physical distance rather than only neighboring original vertices. This suppresses pixel-scale zigzags consistently for dense and sparse outlines, closes the seam periodically, and smooths corners introduced by border buffering. Try 0.3-0.5 mm; zero still preserves the original contour. It can soften intentional sharp details and slightly change the border distance around corners. Export a new PLT after changing it. Mechanical cutting quality still requires a machine trial.

### Multiple image selection
Ctrl+A or Edit > Select all images selects every copy on the sheet. Ctrl+click toggles individual copies. Shift+click selects the range from the last clicked anchor in layout order; Ctrl+Shift+click adds that range. Each selected copy has a blue frame and the property bar reports the selected count. Drag an already selected copy without modifiers to move the whole selection, preserving spacing and clamping the group to page/CCD boundaries. One Undo/Redo restores the entire move. A bounding frame with four corner handles surrounds the selected group. Drag a group corner to scale every selected copy and its relative position; the opposite corner stays fixed. Lock ratio preserves proportions; turning it off allows independent horizontal and vertical scaling. Each copy remains at least 1 mm and the group stays within page/CCD bounds. Numerical object properties appear for a single selected copy. Click blank canvas or Esc to deselect. Space+drag remains canvas navigation.

### Independent grid spacing
Properties > Size & Repeat has Column spacing (horizontal) and Row spacing (vertical), each editable from 0 to 100 mm. Both default to 3 mm. Capacity, preview, print, CCD marks and cut geometry use the same two values. Manually positioned/resized copies retain their overrides; Reflow all copies to grid explicitly resets those edits to the repeat defaults. Large gaps may reduce capacity below the chosen copy count; reduce Copies when the layout reports that it no longer fits.

### Adding copies after manual resize
The original repeat grid no longer blocks a manually resized layout at its old capacity. Existing manual placements are retained. New copies use the resized size when the existing copies all have manual placements, and are packed into free rectangular slots using Column/Row spacing and page margins. Capacity and Fill sheet to capacity now use the current placements. Packing is deterministic and conservative, without rotation or nesting. An actually full sheet still reports insufficient space.

### Keyboard movement
Select one or more copies and use Left/Right/Up/Down to move the selection by 0.1 mm per key press (hold to repeat). Shift+Arrow moves 1 mm. Movement follows page directions regardless of zoom, keeps the group together, and stops at page/CCD safe boundaries. Unselected copies stay fixed. Ctrl+Z/ Ctrl+Y undo/redo moves. Arrow keys keep their normal editing behavior while typing in numeric/text fields and do not move artwork during dialogs or mouse dragging.

### File > Settings and measurement units
File > Settings consolidates General/units, Document/paper, Layout/repeat/spacing, Artwork/background, Cut outline/smoothing, Registration/CCD/calibration, View and Printing Preferences. Display/input units are Inches (default), cm or mm, including New document, object dimensions, rulers and vector output dimensions. Internal geometry and PLT calibration remain in mm; switching units does not resize artwork or change print/cut output. Units per mm is a machine calibration value and is not converted. Arrow and Shift+Arrow distances are configurable.
Changes apply immediately. Save settings stores defaults in %APPDATA%/MC630-PLT-Studio/settings.json for the next launch. It stores settings, not the image or custom placements; use export for production output. Saved repeat defaults must fit the paper. Printer driver preferences continue to use the existing driver-specific storage and dialog.

### Save and reopen projects
File > Save (Ctrl+S) writes the current Print & Cut document to a .mc630 project. First Save asks for a path; Save As (Ctrl+Shift+S) creates another named project. File > Open project (Ctrl+Shift+O), or dragging a .mc630 file onto the Print & Cut canvas, restores it. Ctrl+O continues to open an image.
Projects embed the original image bytes, artwork name, full per-copy placements/sizes, paper/repeat/cut/CCD settings, units, keyboard steps, zoom, view options and pan. Reopening needs no original image file and regenerates the print/cut paths from the embedded artwork. Printer-specific driver preferences remain stored on the local computer; projects do not install or change printer drivers. Undo history and last exported file paths are session-only. Project save writes a temporary sibling and replaces the destination only after writing successfully. Damaged/unsupported projects leave the open document unchanged. New document starts an unnamed project; opening a replacement image edits the current project. Unsaved document changes now prompt Save / Don't Save / Cancel before New, opening another project/image (including drag-and-drop), closing the image or exiting the app. Save must complete successfully before the requested action continues; canceling the Save dialog or a write failure keeps the document open. Navigation and selection alone do not trigger the prompt. The document bar shows * Unsaved.

### Auto Save and crash recovery
Auto Save is enabled by default. File > Settings > Auto Save / crash recovery controls the maximum interval (default 15 seconds, range 5-300). Changes are saved after 2 seconds idle or at the maximum interval during editing; an active mouse drag completes before saving. A background worker writes an atomic, self-contained recovery snapshot, separate from the user project, under %APPDATA%/MC630-PLT-Studio/Recovery. Only one write runs at a time. Changes since the latest successful snapshot may be lost after an abrupt stop.
At startup, an abandoned session offers Restore / Discard recovery. Restore recovers the embedded image, manual layout, document settings and view, remembers the original project path and leaves the document unsaved until you explicitly Save. Invalid-capacity drafts can also be recovered for correction. Recovery does not change global saved preferences or print. Saving, intentionally discarding/exiting, or replacing the document clears obsolete snapshots; a failed Save or canceled close retains protection. Original project files are never overwritten by Auto Save. Windows session locks prevent another running instance from claiming an active recovery. Partial temporary writes are ignored and a failed write preserves the previous valid recovery.

Windows rendering now uses the WGPU backend to avoid the blank OpenGL window observed during recovery testing. Printer discovery runs in the background so unavailable network printers do not hold up the workspace/recovery prompt. The final build was tested with an actual abrupt stop of a 20-image, manually resized sample session; restarting showed recovery and restored all 20 custom placements.

### File > Recent Projects
The last 10 successfully opened or saved projects are stored in %APPDATA%/MC630-PLT-Studio/recent-projects.json and restored on startup. Recent entries are ordered newest first, with duplicates removed (case-insensitive on Windows). Hover an entry for its full path. Selecting one uses the same unsaved-change protection as Open Project. Missing or damaged projects show an error while retaining the current document and are not promoted to the top. Clear recent history clears only the list, never project files. Auto Save recovery files are excluded from this list.

### Reopen the last project at startup
File > Settings > General / measurement units has Reopen last project on startup (off by default). Enable it and click Save settings. On the next launch the newest successfully opened/saved entry in Recent Projects opens automatically. A pending crash-recovery offer takes priority and suppresses automatic project reopening for that launch. Missing or damaged files leave the workspace open with an explanatory status message. The option belongs to the app: opening another project or restoring recovery never changes this startup preference. Changing the option alone does not mark the document unsaved.

### Double-click .mc630 projects on Windows
The app accepts a .mc630 path as a command-line argument, including spaces and Unicode names. Register the per-user file association with MC630-PLT-Studio.exe --register-file-association, or File > Settings > General > Associate .mc630 project files with this app. No administrator rights are needed. The handler command quotes both the executable and the project argument. Registration advertises the app to Open With/default-app settings and does not change a protected Windows UserChoice. If Windows asks for a default application, choose MC630 PLT Studio. Re-register after moving the executable.
An explicitly launched project takes priority over Reopen last project. Pending recovery is offered first; if recovered unsaved work would be replaced, the existing Save / Don't Save / Cancel protection applies. Double-click opens another app window when one is already running, preserving the existing window and its unsaved work.

Rulers adapt their subdivisions to zoom: binary inch subdivisions and decimal cm/mm ticks keep readable labels and reveal finer measurements at higher zoom. Ruler drawing is limited to visible ticks; print and PLT dimensions are unaffected.

Multiple projects: Open project / Recent Projects open independent tabs. New and + New create a tab; click a tab to switch. Close the active project with File > Close project, Ctrl+W, or its tab close button. Each tab retains its own image, layout, settings, navigation, undo and Auto Save. Unsaved changes are checked per project on tab close and across every tab on application exit. Opening an already open project selects its tab. Closing the last tab leaves an empty Untitled document.

Startup tab restoration: Settings > General > Reopen all project tabs on startup restores saved .mc630 tabs and the selected tab from workspace-session.json in the user settings folder. Open/close/switch/Save As update the session; application exit preserves the full tab list after all save decisions. Missing or invalid projects are skipped with a status message. Unsaved documents and edits remain under Auto Save recovery; ordinary session restoration does not overwrite project files.

File > Save All saves all open Print & Cut project tabs in tab order. Named projects update their files; untitled tabs prompt for a .mc630 path. Cancel or a failed save stops the operation at that tab and reports the saved count, leaving every tab open. Successful completion restores the originally selected tab and updates the startup session with any newly assigned paths.

File > Close All asks Save / Don't Save / Cancel for each modified project, keeps the application open, and leaves one empty Untitled document after completion. Cancel or a failed/canceled Save stops the operation and retains all tabs, including previously approved tabs. Completed Close All clears the startup tab list; application Exit still preserves it.

Project tab context menu: right-click any tab for Save, Close, Close Others, and Close All. Save and Close apply to the clicked tab. Close Others retains that tab with its current image, layout, settings and edits; other modified tabs use the normal Save / Don't Save / Cancel prompts. Cancel keeps every tab open. Close Others is disabled when only one tab is open.

Tab ordering: hold the left mouse button on a project tab, drag horizontally to the insertion marker, and release to reorder. Escape or dropping outside the tab bar cancels. Tab movement preserves document state and the selected project. Saved project tab order is persisted immediately and restored on startup when Reopen all project tabs is enabled.

Tab keyboard navigation: Ctrl+Tab selects the next project tab; Ctrl+Shift+Tab selects the previous tab, wrapping at both ends in the current tab order. The selected tab is remembered for startup. Navigation preserves edits and is paused during save/close operations, recovery/unsaved prompts, New/Settings dialogs, and tab dragging.

Classic workspace: PageMaker-inspired neutral grey controls, square buttons, menu and standard toolbar above document tabs, two-column monochrome toolbox, and a bottom control palette. Properties / Print panels are optional (toolbar or View > Properties palette). View also provides margin guides and preview contour colours; these guides never print or export. Switch to Vector document workspace through View; its menu returns to Print & Cut. Existing project tabs, shortcuts, print settings and PLT generation are preserved.

Floating Properties palette: drag its title bar to reposition it within the workspace, resize from its edges/corner, collapse using the title arrow, or close it using X. Properties and Print / Export share the same floating window and position. Reopen from the Properties toolbar button or View menu. Palette movement, size and tab choice do not change print or cut geometry; canvas pan/zoom ignores the floating palette.

Palette persistence: position, inner size, visibility and Properties/Print tab are saved automatically in palette-state.json in the user settings folder after mouse release. Startup restores that global workspace state independently of project files. Geometry is constrained to the available workspace on smaller windows, and content size is stored separately from the title/frame to prevent growth on repeated launches.

Refined components: shared light theme, consistent 28px controls, 4px button/input corners, 6px white section cards, aligned numeric inputs and equal palette tabs. Animation is disabled, layout capacity is cached until changes, and numeric text commits on Enter/focus loss to avoid recalculating incomplete input.

Properties cards: click Artwork, Size and repeat or Cut outline headings to independently collapse/expand them. All start expanded; current-session state survives palette hide/show and project tab switching. Collapsed sections retain their settings.

Independent property dialogs: Properties now launches Artwork, Size and Repeat, and Cut Outline as three separate movable/resizable windows. Open or hide each from its own launcher button, or close it with X. All three can remain open together, including when the Properties launcher is hidden. Each dialog's visibility, position and size are stored independently in palette-state.json and restored at startup; legacy palette settings remain compatible.

Direct toolbar dialog buttons: Artwork, Size and Repeat, and Cut Outline replace the Properties toolbar button. Click each to show/hide its independent dialog; selected button state indicates an open dialog.

Performance: physical outline geometry now persists across preview layout changes. Moving/rearranging copies reuses smoothing/border results instead of recomputing them. Cache invalidation covers artwork, size, smoothing, border, holes and validation settings; at most 64 outline variants are retained. A manual ignored UI profiling test (profile_ui_latency) measures headless UI update+tessellation for 20 repeated stickers; measured movement dropped from ~9.7 ms/frame to ~0.9 ms/frame on this machine. This is a CPU test, not an end-to-end GPU/input latency guarantee.

Custom page size: choose Layout > Page size or click the paper-size button on the bottom control bar. Enter width and height in mm, cm or inches, choose a preset, or swap dimensions. Width supports 50-630 mm and height supports 50-2000 mm. Existing image placements stay fixed; sizes that would clip images, manual cuts, guides or CCD marks are rejected. Apply supports Undo/Redo, project persistence, printing and matching PLT export.
