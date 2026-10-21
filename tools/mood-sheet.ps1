# Two modes:
#  -Dir <dir> [-Moods a,b,...]   Stitches <dir>\<theme>__<mood>.png (from mood-shots.mjs) into a grid: one row per theme,
#                                one column per mood/expression. Output stays in <dir> (moods-N.png).
#  -Grid <json> [-Out <png>]     Draws the pixel grid written by mood-grid.mjs (all expressions x all themes, no app needed),
#                                each sprite at -Scale on its theme background. Output: <json dir>\expressions.png by default.
param([string]$Dir, [string[]]$Moods = @('idle','ai','session','break','error'), [int]$PerSheet = 7,
      [string]$Grid, [string]$Out, [int]$Scale = 5)
Add-Type -AssemblyName System.Drawing
$Moods = @($Moods | ForEach-Object { $_ -split ',' } | Where-Object { $_ }) # "-Moods a,b" arrives as one string via -File

if ($Grid) {
    $data = Get-Content $Grid -Raw | ConvertFrom-Json
    $exprs = $data.expressions; $themes = $data.themes
    $cell = 16 * $Scale + 8; $labelW = 80; $headH = 20
    $sheet = New-Object Drawing.Bitmap ($labelW + $cell * $exprs.Count), ($headH + $cell * $themes.Count)
    $g = [Drawing.Graphics]::FromImage($sheet); $g.Clear([Drawing.Color]::White)
    $font = New-Object Drawing.Font 'Segoe UI', 8
    for ($c = 0; $c -lt $exprs.Count; $c++) { $g.DrawString($exprs[$c], $font, [Drawing.Brushes]::Black, $labelW + $c * $cell + 2, 3) }
    $brushes = @{}
    for ($r = 0; $r -lt $themes.Count; $r++) {
        $t = $themes[$r]; $y0 = $headH + $r * $cell
        $g.DrawString($t.id, $font, [Drawing.Brushes]::Black, 4, $y0 + $cell / 2 - 8)
        $bg = New-Object Drawing.SolidBrush ([Drawing.ColorTranslator]::FromHtml($t.bg))
        for ($c = 0; $c -lt $exprs.Count; $c++) {
            $x0 = $labelW + $c * $cell
            $g.FillRectangle($bg, $x0 + 1, $y0 + 1, $cell - 2, $cell - 2)
            $rows = $t.frames.($exprs[$c])
            for ($y = 0; $y -lt 16; $y++) { for ($x = 0; $x -lt 16; $x++) {
                $col = $rows[$y][$x]; if (-not $col) { continue }
                if (-not $brushes.ContainsKey($col)) { $brushes[$col] = New-Object Drawing.SolidBrush ([Drawing.ColorTranslator]::FromHtml($col)) }
                $g.FillRectangle($brushes[$col], $x0 + 4 + $x * $Scale, $y0 + 4 + $y * $Scale, $Scale, $Scale)
            } }
        }
        $bg.Dispose()
    }
    if (-not $Out) { $Out = Join-Path (Split-Path $Grid) 'expressions.png' }
    $g.Dispose(); $sheet.Save($Out); $sheet.Dispose()
    "$Out : $($themes.Count) themes x $($exprs.Count) expressions"
    return
}

$themes = Get-ChildItem $Dir -Filter "*__$($Moods[0]).png" | ForEach-Object { $_.Name -replace "__$($Moods[0])\.png$",'' } | Sort-Object
$sample = [Drawing.Image]::FromFile((Join-Path $Dir "$($themes[0])__$($Moods[0]).png")); $w = $sample.Width; $h = $sample.Height; $sample.Dispose()
$labelW = 90
for ($s = 0; $s * $PerSheet -lt $themes.Count; $s++) {
    $chunk = @($themes[($s * $PerSheet)..([math]::Min(($s + 1) * $PerSheet, $themes.Count) - 1)])
    $sheet = New-Object Drawing.Bitmap ($labelW + $w * $Moods.Count), ($h * $chunk.Count + 22)
    $g = [Drawing.Graphics]::FromImage($sheet); $g.Clear([Drawing.Color]::White)
    $font = New-Object Drawing.Font 'Segoe UI', 10
    for ($c = 0; $c -lt $Moods.Count; $c++) { $g.DrawString($Moods[$c], $font, [Drawing.Brushes]::Black, $labelW + $c * $w + 4, 3) }
    for ($r = 0; $r -lt $chunk.Count; $r++) {
        $g.DrawString($chunk[$r], $font, [Drawing.Brushes]::Black, 4, 22 + $r * $h + $h / 2 - 8)
        for ($c = 0; $c -lt $Moods.Count; $c++) {
            $p = Join-Path $Dir "$($chunk[$r])__$($Moods[$c]).png"
            if (-not (Test-Path $p)) { continue }
            $img = [Drawing.Image]::FromFile($p)
            $g.DrawImage($img, $labelW + $c * $w, 22 + $r * $h, $w, $h); $img.Dispose()
        }
    }
    $g.Dispose(); $sheet.Save((Join-Path $Dir "moods-$($s + 1).png")); $sheet.Dispose()
    "moods-$($s + 1).png : $($chunk -join ', ')"
}
