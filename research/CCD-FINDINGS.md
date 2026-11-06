# MC630 CCD research, 2026-10-23

## Verified observations

- The manufacturer's MC User Manual page links to a public Drive folder containing the MC **CCD Camera Type** manual and `1.6 Contour Cut from U-Disc.mp4`.
- In that video at 275 and 305 seconds, the selected device is **MC630**, mode **Automatic ARMS / CCD**. The print preview shows four filled black **circles**, one at each corner of a rectangular mark area. These are different from the manual crosses in this app.
- The video shows `Mark Position 5.5 mm`, and `Marks: 186.539 mm x 233.485 mm`. **Mark Position is not documented as dot diameter**; it must not be represented as a firmware diameter specification.
- The accompanying MC manual describes selecting the PLT in the U-disc menu, setting repeats/spacing, and starting camera scan/cutting. It does not publish the PLT camera protocol or mark dimensions.
- The user's working PLT begins `TB25,10092,7593;CT1;IN;PA;`.
- An independent Inkcut issue describes `TB25` with registration-mark spans. Its linked old extension is for **VicSign**, not MC630; that implementation cannot certify this firmware. It also shows that axis order can differ across implementations.

## Unverified trial implementation

The opt-in **CCD 4-dot trial (EXPERIMENTAL)** uses diameter 5.5 mm, center inset 7 mm, 5 mm artwork clearance, four corner circles, mark spans `(210-2*inset, 297-2*inset)` in `TB25`, and cut paths relative to the lower-left mark. Both dimensions are user-adjustable trial values. **No source explicitly specifies these values for MC630 Ver5.022.**

The first-dot corner, feed orientation, units/mm, TB25 argument order and CT1 meaning remain unverified for Ver5.022. The software does not control the camera. The trial only generates candidate print/PLT files for a blade-removed physical test; it is not validated production CCD support. The camera may reject them. `CCD_TEST.plt` contains the full contours and can start motion after scanning.

Manual mode remains the default and keeps its existing export behavior. No proprietary installer was installed or executed. No GPL extension code was copied into the Rust app; its protocol observations are corroboration only.

## Sources

- [Official MC manual page](https://www.mycutcutter.com/info/new-ccd-camear-contour-cut-pluginin-for-coreld-45966569.html)
- [Official linked MC files](https://drive.google.com/drive/folders/115yLaMGyXySj8F0NMulpEhRF76zku1Ay)
- [Official linked U-disc video](https://drive.google.com/file/d/1esxtXBbMhAsvkv9v1Tidgz5UEc8SaPzX/view)
- [Official SignMaster download/guide page](https://www.mycutcutter.com/info/signmaster-software-download-for-mycut-vinyl-c-19911727.html)
- [Inkcut protocol discussion](https://github.com/inkcut/inkcut/issues/379)

Local evidence: `mc-ccd-manual.pdf`, `official-udisc-contour.mp4`, `official-275.jpg`, `official-detail-305.jpg`. These are source material, not executable instructions.

2026-10-23 user physical observation: first top-left dot acquired; next search travels toward bottom-left but stops short. Existing TB25=6520,8040 represented horizontal 163 mm then vertical 201 mm at assumed40 units/mm. Prepared isolated TB25=8040,6520 trial, no cut coordinate or print changes. This is evidence for feed-first parameter order, pending physical confirmation; app default has not been changed.

Physical 4-column x 5-row print produced 5-column x 4-row cut and transposed asymmetric shapes. CCD export now transposes each PU/PD point to feed Y, carriage X; scan TB25 stays feed-first. Page preview and print remain unchanged. New output: examples/CCD-AXES-FIXED/CUT.plt. Hardware alignment pending user test. Copy this CCD file directly; do not re-import/re-export via SVG/PLT tools because that normalizes origin and regenerates bounds.
