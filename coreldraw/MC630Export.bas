Attribute VB_Name = "MC630Export"
Option Explicit

Private Const ConverterPath As String = "F:\Cutter\target\release\plt-convert.exe"
Private Const UnitsPerMillimeter As String = "40"

Public Sub ExportSelectionToMC630()
    Dim doc As Document
    Dim svgFilter As ExportFilter
    Dim fs As Object, runner As Object
    Dim folder As String, svgPath As String, outputPath As String
    Dim width As Double, cmd As String, exitCode As Long
    Dim failure As String, ownsFolder As Boolean

    On Error GoTo Failed
    If Documents.Count = 0 Then Err.Raise vbObjectError + 1, , "Open a design first."
    Set doc = ActiveDocument
    If ActiveSelectionRange.Count = 0 Then Err.Raise vbObjectError + 2, , "Select the vector objects to cut."
    width = Application.ConvertUnits(ActiveSelectionRange.SizeWidth, doc.Unit, cdrMillimeter)
    If width <= 0 Then Err.Raise vbObjectError + 3, , "The selection must have positive width."

    Set fs = CreateObject("Scripting.FileSystemObject")
    If Not fs.FileExists(ConverterPath) Then Err.Raise vbObjectError + 4, , "Build plt-convert first. See coreldraw/README.md."
    Set runner = CreateObject("WScript.Shell")
    outputPath = Trim$(InputBox("Enter a full path for a new PLT file.", "MC630 PLT Export", _
        fs.BuildPath(runner.SpecialFolders("Desktop"), "MC630-CUT.plt")))
    If Len(outputPath) = 0 Then Exit Sub
    If LCase$(Right$(outputPath, 4)) <> ".plt" Then outputPath = outputPath & ".plt"
    If Mid$(outputPath, 2, 2) <> (":" & Chr$(92)) Then Err.Raise vbObjectError + 5, , "Use a full local or USB drive path."
    If fs.FileExists(outputPath) Then Err.Raise vbObjectError + 6, , "That file exists. Choose a new name."

    folder = fs.BuildPath(fs.GetSpecialFolder(2).Path, "mc630-" & fs.GetTempName)
    fs.CreateFolder folder
    ownsFolder = True
    svgPath = fs.BuildPath(folder, "selection.svg")
    Set svgFilter = doc.ExportEx(svgPath, cdrSVG, cdrSelection)
    svgFilter.Finish

    cmd = QuoteArg(ConverterPath) & " " & QuoteArg(svgPath) & " " & QuoteArg(outputPath) & _
        " " & Trim$(Str$(width)) & " " & UnitsPerMillimeter & " --plain"
    exitCode = runner.Run(cmd, 0, True)
    If exitCode <> 0 Then Err.Raise vbObjectError + 7, , _
        "Conversion failed. Check that the selection contains vector curves, the output folder exists, and the destination is writable."
    MsgBox "Saved " & outputPath, vbInformation, "MC630 PLT Export"
    GoTo CleanUp

Failed:
    failure = Err.Description
    MsgBox failure, vbExclamation, "MC630 PLT Export"
CleanUp:
    On Error Resume Next
    If Len(svgPath) > 0 Then
        If fs.FileExists(svgPath) Then fs.DeleteFile svgPath
    End If
    If ownsFolder Then
        If fs.FolderExists(folder) Then
            If fs.GetFolder(folder).Files.Count = 0 And fs.GetFolder(folder).SubFolders.Count = 0 Then fs.DeleteFolder folder
        End If
    End If
End Sub

Private Function QuoteArg(ByVal value As String) As String
    If InStr(value, Chr$(34)) > 0 Or InStr(value, "%") > 0 Then
        Err.Raise vbObjectError + 8, , "Paths containing quotes or percent signs are unsupported."
    End If
    QuoteArg = Chr$(34) & value & Chr$(34)
End Function
