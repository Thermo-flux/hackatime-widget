# Stitches <dir>\<theme>-<tab>.png screenshots into one contact sheet per tab. Output stays in <dir>.
# Usage: .\contact-sheet.ps1 -Dir D:\dev\tmp\shots -Tabs stats,sessions,pomodoro -Cols 4 -Scale 0.6
param([string]$Dir, [string[]]$Tabs = @('stats'), [int]$Cols = 4, [double]$Scale = 0.6)
Add-Type -AssemblyName System.Drawing
foreach ($tab in $Tabs) {
    $files = Get-ChildItem $Dir -Filter "*-$tab.png" | Sort-Object Name
    if (-not $files) { continue }
    $first = [Drawing.Image]::FromFile($files[0].FullName)
    $w = [int]($first.Width * $Scale); $h = [int]($first.Height * $Scale); $first.Dispose()
    $rows = [math]::Ceiling($files.Count / $Cols)
    $sheet = New-Object Drawing.Bitmap ($w * $Cols), ($h * $rows)
    $g = [Drawing.Graphics]::FromImage($sheet)
    $g.Clear([Drawing.Color]::FromArgb(128, 128, 128))
    $g.InterpolationMode = 'HighQualityBicubic'
    for ($i = 0; $i -lt $files.Count; $i++) {
        $img = [Drawing.Image]::FromFile($files[$i].FullName)
        $g.DrawImage($img, ($i % $Cols) * $w, [math]::Floor($i / $Cols) * $h, $w, $h)
        $img.Dispose()
    }
    $g.Dispose()
    $sheet.Save((Join-Path $Dir "sheet-$tab.png"))
    $sheet.Dispose()
    "sheet-$tab.png : $($files.Count) themes"
}
