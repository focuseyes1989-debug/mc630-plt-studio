# CorelDRAW X7 (64-bit) to MC630

This macro exports selected vector objects to a USB-ready PLT file using the Cutter converter. This first version is for ordinary vector cutting. It does not generate registration dots or CCD print-and-cut jobs. Continue using the existing Studio CCD workflow for those jobs.

1. In F:\Cutter run: cargo build --release --locked --bin plt-convert
2. In CorelDRAW X7 open Tools > Macros > Macro Manager. Create a global macro project or use an existing writable global project. In the VBA editor, select that project and use File > Import File to import MC630Export.bas. Save the project. If the VBA editor is unavailable, CorelDRAW's VBA component must be working before the macro can be imported.
3. Select the vector outlines you want to cut. Convert text to curves before export. Run MC630Export.ExportSelectionToMC630 in Macro Manager. You can assign that macro to a toolbar button via Tools > Customization > Commands > Macros.
4. Enter a new full .plt path, such as E:\CUT001.plt. Use the cutter's USB file menu to run the job. The output starts at the selected paths' bounds rather than the CorelDRAW page origin.

The macro uses 40 PLT units/mm, inferred from the supplied CutTool samples. Verify with a 20 mm square on scrap or with a pen before production. The output follows the non-marked test1.plt command envelope. CorelDRAW macro import and physical cutting require local verification.
