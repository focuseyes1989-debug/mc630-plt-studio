# MC630 PLT Studio v0.1.13

## Multiple pages in one project
- Add up to 100 pages to one document. Each page retains independent Main/Back artwork, designs, cutlines, paper size and layout settings.
- Use the Pages bar above Status to select, add, duplicate or delete pages. New pages follow the current page; deletion requires confirmation and the last page is retained.
- Save/Open and autosave recovery include every page and the active page index. Page navigation retains in-session Undo/Redo history and does not mark the document edited.
- Prepare inactive pages on a background worker with progress; retain source data without keeping their GPU textures loaded.

## Page range printing and progress
- Choose Current, All or Range in Print. Enter page numbers and ranges such as `1, 3, 5-8`; requested order is preserved and duplicate pages are removed.
- Print the chosen Main or Back side for each selected page in one Windows print job. Invalid ranges disable Print, and every selected paper size is checked against printer preferences before submission.
- Process one page raster at a time, including blank pages.
- Replace the print loading circle with a compact progress bar, percentage and preparing/sending/submitting stages. 100% means submission to the Windows printer queue, not physical printing completion.

## Keyboard movement
- Move selected Design shapes, standalone Cutline paths, or mixed selections with images using the arrow keys.
- Arrow uses the fine step (default 0.1 mm); Shift uses coarse (1 mm); Ctrl multiplies the step by 10; Alt divides it by 10. Modifiers combine and movement is independent of zoom.
- Preserve styles and relative positions, respect page/CCD bounds and support Undo/Redo. Text input, popup/modal controls and active drawing/transform gestures retain their keyboard input.

## Compatibility
- Existing single-page projects remain supported. Multi-page files use project format version 2 and require this update; older Studio versions reject them to avoid losing pages on save.
- Project and recovery files support up to 256 MB total serialized data, with existing per-image limits retained.
- HTML and PLT export still operate on the selected page. Pages printed together must match the printer's paper size and orientation.

Validation: 220 automated tests passed; 9 manual/device/performance diagnostics remain ignored. Additional local virtual XPS testing verified three pages in one document and print-progress milestones.

Download MC630-PLT-Studio.exe, verify it with SHA256SUMS.txt, and retain THIRD-PARTY-NOTICES.txt. Existing installations can use Studio updates.
