# drive.ps1 — UI Automation driver for the Nebula Slint GUI (Windows).
#
# Dot-source this from a scenario script and run the scenario under
# *Windows* PowerShell in STA mode, because SendKeys needs an STA thread:
#
#   powershell.exe -NoProfile -ExecutionPolicy Bypass -STA -File scenario.ps1
#
# The file must stay UTF-8 **with BOM**: Windows PowerShell 5.1 reads a
# BOM-less UTF-8 file as ANSI and every Korean accessible-label stops matching.
#
# Slint (accesskit) maps `accessible-label` to the UIA Name property and
# `accessible-role` to ControlType. It sets no AutomationId anywhere in this
# app, so Name is the only stable handle.

$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8

Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
Add-Type -AssemblyName System.Drawing
Add-Type -AssemblyName System.Windows.Forms

if (-not ("NebulaWin32" -as [type])) {
    Add-Type -TypeDefinition @'
using System;
using System.Text;
using System.Runtime.InteropServices;
public class NebulaWin32 {
    [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr hwnd, IntPtr hdc, uint flags);
    [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr hwnd, out RECT r);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hwnd, out RECT r);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hwnd);
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr hwnd, int cmd);
    [DllImport("user32.dll")] public static extern bool IsIconic(IntPtr hwnd);
    [DllImport("user32.dll")] public static extern IntPtr PostMessage(IntPtr h, uint m, IntPtr w, IntPtr l);
    [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr p);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetClassName(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowTextW(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] public static extern void mouse_event(uint f, uint x, uint y, uint d, IntPtr e);
    public delegate bool EnumProc(IntPtr h, IntPtr p);
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
    public const uint WM_CLOSE = 0x0010;
    public const uint MOUSEEVENTF_LEFTDOWN = 0x0002;
    public const uint MOUSEEVENTF_LEFTUP = 0x0004;
}
'@
}

$script:AE   = [System.Windows.Automation.AutomationElement]
$script:Cond = [System.Windows.Automation.PropertyCondition]
$script:Scope = [System.Windows.Automation.TreeScope]
$script:CT   = [System.Windows.Automation.ControlType]

$script:Win      = $null      # AutomationElement of the app window
$script:Hwnd     = [IntPtr]::Zero
$script:ShotDir  = $null
$script:LogPath  = $null
$script:ShotSeq  = 0
$script:Failures = @()

# ── logging ────────────────────────────────────────────────────────────────

function Start-Log([string] $Dir, [string] $Name) {
    $script:ShotDir = $Dir
    New-Item -ItemType Directory -Force -Path $Dir | Out-Null
    $script:LogPath = Join-Path $Dir "$Name.log"
    Set-Content -LiteralPath $script:LogPath -Value "# $Name  $(Get-Date -Format 'yyyy-MM-dd HH:mm:ss')" -Encoding UTF8
}

function Say([string] $Text) {
    Write-Host $Text
    if ($script:LogPath) { Add-Content -LiteralPath $script:LogPath -Value $Text -Encoding UTF8 }
}

function Check([string] $What, [bool] $Ok, [string] $Detail = '') {
    $mark = if ($Ok) { 'PASS' } else { 'FAIL' }
    Say ("[{0}] {1}{2}" -f $mark, $What, $(if ($Detail) { " -- $Detail" } else { '' }))
    if (-not $Ok) { $script:Failures += $What }
}

# ── window ─────────────────────────────────────────────────────────────────

# winit gives the process more than one top-level window (there is a 16x16
# helper with no title), so match on the title and a real size rather than
# taking the first child of the desktop.
function Connect-App([int] $ProcessId, [int] $TimeoutSeconds = 40, [string] $Title = 'Nebula') {
    $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    while ((Get-Date) -lt $deadline) {
        $all = @($script:AE::RootElement.FindAll(
            $script:Scope::Children,
            (New-Object $script:Cond($script:AE::ProcessIdProperty, $ProcessId))))
        foreach ($w in $all) {
            $r = $w.Current.BoundingRectangle
            if ($w.Current.Name -eq $Title -and $r.Width -gt 200 -and $r.Height -gt 200) {
                $script:Win = $w
                $script:Hwnd = [IntPtr]$w.Current.NativeWindowHandle
                return $w
            }
        }
        Start-Sleep -Milliseconds 250
    }
    throw "no '$Title' UIA window for pid $ProcessId after $TimeoutSeconds s"
}

function Get-Win { if (-not $script:Win) { throw 'Connect-App first' }; $script:Win }

# SetForegroundWindow is refused when the caller is not itself the foreground
# process, and SendKeys then types into whatever window *is* — which silently
# skips a whole scenario. Verify, and fall back to a click on the inert page
# title strip, which always raises the window.
function Activate-Win([switch] $NoClick) {
    if ([NebulaWin32]::IsIconic($script:Hwnd)) { [void][NebulaWin32]::ShowWindow($script:Hwnd, 9) }
    for ($try = 0; $try -lt 6; $try++) {
        [void][NebulaWin32]::SetForegroundWindow($script:Hwnd)
        Start-Sleep -Milliseconds 150
        if ([NebulaWin32]::GetForegroundWindow() -eq $script:Hwnd) { return $true }
        if ($NoClick) { continue }
        $r = New-Object NebulaWin32+RECT
        [void][NebulaWin32]::GetWindowRect($script:Hwnd, [ref] $r)
        [void][NebulaWin32]::SetCursorPos($r.Left + 300, $r.Top + 95)   # page-title strip: no control there
        Start-Sleep -Milliseconds 60
        [NebulaWin32]::mouse_event([NebulaWin32]::MOUSEEVENTF_LEFTDOWN, 0, 0, 0, [IntPtr]::Zero)
        Start-Sleep -Milliseconds 50
        [NebulaWin32]::mouse_event([NebulaWin32]::MOUSEEVENTF_LEFTUP, 0, 0, 0, [IntPtr]::Zero)
        Start-Sleep -Milliseconds 200
    }
    $false
}

# Navigate by clicking the nav rail and assert the page actually changed.
function Goto-Page([string] $Page) {
    for ($try = 0; $try -lt 3; $try++) {
        if ((Get-PageTitle) -eq $Page) { return $true }
        $item = Find-Control -Name $Page -Type TabItem
        [void](Invoke-Control $item)
        Start-Sleep -Milliseconds 600
    }
    $ok = (Get-PageTitle) -eq $Page
    if (-not $ok) { Say "  !! Goto-Page '$Page' failed; title is [$(Get-PageTitle)]" }
    $ok
}

function Get-WindowRects {
    $wr = New-Object NebulaWin32+RECT
    $cr = New-Object NebulaWin32+RECT
    [void][NebulaWin32]::GetWindowRect($script:Hwnd, [ref] $wr)
    [void][NebulaWin32]::GetClientRect($script:Hwnd, [ref] $cr)
    [pscustomobject]@{
        WindowW = $wr.Right - $wr.Left; WindowH = $wr.Bottom - $wr.Top
        ClientW = $cr.Right - $cr.Left; ClientH = $cr.Bottom - $cr.Top
        Left = $wr.Left; Top = $wr.Top
    }
}

# ── find ───────────────────────────────────────────────────────────────────

function Resolve-Type([string] $Type) {
    switch ($Type) {
        'Button'      { $script:CT::Button }
        'Edit'        { $script:CT::Edit }
        'CheckBox'    { $script:CT::CheckBox }
        'Text'        { $script:CT::Text }
        'TabItem'     { $script:CT::TabItem }
        'Tab'         { $script:CT::Tab }
        'ListItem'    { $script:CT::ListItem }
        'List'        { $script:CT::List }
        'Group'       { $script:CT::Group }
        'ProgressBar' { $script:CT::ProgressBar }
        default       { throw "unknown -Type $Type" }
    }
}

# All descendants whose Name equals -Name (optionally filtered by role).
function Find-Controls([string] $Name, [string] $Type) {
    $c = New-Object $script:Cond($script:AE::NameProperty, $Name)
    if ($Type) {
        $t = New-Object $script:Cond($script:AE::ControlTypeProperty, (Resolve-Type $Type))
        $c = New-Object System.Windows.Automation.AndCondition($c, $t)
    }
    @((Get-Win).FindAll($script:Scope::Descendants, $c))
}

# The single control with that Name (and role). -Index picks among duplicates.
function Find-Control([string] $Name, [string] $Type, [int] $Index = 0, [int] $TimeoutSeconds = 6, [switch] $Optional) {
    $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    do {
        $all = Find-Controls -Name $Name -Type $Type
        if ($all.Count -gt $Index) { return $all[$Index] }
        Start-Sleep -Milliseconds 150
    } while ((Get-Date) -lt $deadline)
    if ($Optional) { return $null }
    throw "control not found: Name='$Name' Type='$Type' Index=$Index"
}

# Any descendant whose Name *contains* the fragment.
function Find-ByPartialName([string] $Fragment, [string] $Type, [int] $TimeoutSeconds = 6, [switch] $Optional) {
    $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    do {
        $cond = if ($Type) { New-Object $script:Cond($script:AE::ControlTypeProperty, (Resolve-Type $Type)) }
                else { [System.Windows.Automation.Condition]::TrueCondition }
        foreach ($e in (Get-Win).FindAll($script:Scope::Descendants, $cond)) {
            if ($e.Current.Name -and $e.Current.Name.Contains($Fragment)) { return $e }
        }
        Start-Sleep -Milliseconds 150
    } while ((Get-Date) -lt $deadline)
    if ($Optional) { return $null }
    throw "no control whose name contains '$Fragment' (Type='$Type')"
}

# ── act ────────────────────────────────────────────────────────────────────

function Invoke-Control($Element) {
    if (-not $Element) { throw 'Invoke-Control: null element' }
    $p = $null
    if ($Element.TryGetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern, [ref] $p)) {
        $p.Invoke(); return 'Invoke'
    }
    if ($Element.TryGetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern, [ref] $p)) {
        $p.Toggle(); return 'Toggle'
    }
    if ($Element.TryGetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern, [ref] $p)) {
        $p.Select(); return 'SelectionItem'
    }
    Click-Control $Element
    'Click'
}

# Physical click at the element's centre — the fallback when a role exposes no
# pattern (Slint's tab / list-item roles).
function Click-Control($Element) {
    [void](Activate-Win)
    $r = $Element.Current.BoundingRectangle
    if ($r.Width -le 0 -or $r.Height -le 0) { throw "element '$($Element.Current.Name)' has no on-screen rect" }
    $x = [int]($r.Left + $r.Width / 2)
    $y = [int]($r.Top + $r.Height / 2)
    [void][NebulaWin32]::SetCursorPos($x, $y)
    Start-Sleep -Milliseconds 80
    [NebulaWin32]::mouse_event([NebulaWin32]::MOUSEEVENTF_LEFTDOWN, 0, 0, 0, [IntPtr]::Zero)
    Start-Sleep -Milliseconds 60
    [NebulaWin32]::mouse_event([NebulaWin32]::MOUSEEVENTF_LEFTUP, 0, 0, 0, [IntPtr]::Zero)
    Start-Sleep -Milliseconds 200
}

# Mouse-wheel the page. Slint's ScrollView exposes no UIA Scroll pattern, and
# content scrolled outside its viewport is dropped from the accessibility tree,
# so a driver has to physically scroll before it can read the lower cards.
function Scroll-Page([int] $Notches, [int] $AtX = 400, [int] $AtY = 500) {
    [void](Activate-Win)
    $r = Get-WindowRects
    [void][NebulaWin32]::SetCursorPos($r.Left + $AtX, $r.Top + $AtY)
    Start-Sleep -Milliseconds 100
    # Positive $Notches scrolls the page down, which is a *negative* WHEEL delta.
    $step = if ($Notches -gt 0) { -120 } else { 120 }
    # Windows PowerShell parses 0xFFFFFFFF as Int32 -1, so wrap by hand.
    $raw = if ($step -lt 0) { [uint32](4294967296 + $step) } else { [uint32]$step }
    for ($i = 0; $i -lt [Math]::Abs($Notches); $i++) {
        [NebulaWin32]::mouse_event(0x0800, 0, 0, $raw, [IntPtr]::Zero)  # MOUSEEVENTF_WHEEL
        Start-Sleep -Milliseconds 60
    }
    Start-Sleep -Milliseconds 300
}

function Set-Text($Element, [string] $Value) {
    $p = $null
    if ($Element.TryGetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern, [ref] $p)) {
        $p.SetValue($Value); Start-Sleep -Milliseconds 150; return 'ValuePattern'
    }
    # Fallback: focus the field, select all, retype.
    Click-Control $Element
    [System.Windows.Forms.SendKeys]::SendWait('^a')
    Start-Sleep -Milliseconds 80
    [System.Windows.Forms.SendKeys]::SendWait((Escape-Keys $Value))
    Start-Sleep -Milliseconds 150
    'SendKeys'
}

# Types like a user: focus, select all, then real keystrokes. Slint's `edited`
# callback is what arms the host's 300 ms settings debounce, so a settings field
# must be typed into rather than written through ValuePattern.
function Type-Text($Element, [string] $Value) {
    Click-Control $Element
    [System.Windows.Forms.SendKeys]::SendWait('^a')
    Start-Sleep -Milliseconds 80
    [System.Windows.Forms.SendKeys]::SendWait('{DEL}')
    Start-Sleep -Milliseconds 80
    if ($Value -ne '') {
        [System.Windows.Forms.SendKeys]::SendWait((Escape-Keys $Value))
    }
    Start-Sleep -Milliseconds 250
}

function Escape-Keys([string] $s) {
    ($s -replace '([+^%~(){}\[\]])', '{$1}')
}

# The ValuePattern value, or $null when the element has no ValuePattern.
# Distinct from Get-Text: an *empty* field must not fall back to its Name, or
# "is this field still blank?" silently answers with the label.
function Get-Value($Element) {
    if (-not $Element) { return $null }
    $p = $null
    if ($Element.TryGetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern, [ref] $p)) {
        return [string]$p.Current.Value
    }
    $null
}

function Get-Text($Element) {
    if (-not $Element) { return $null }
    $v = Get-Value $Element
    if ($null -ne $v) { return $v }
    $Element.Current.Name
}

# UIA FullDescription (property id 30159) is where AccessKit puts an element's
# `description`; System.Windows.Automation has no strongly-typed accessor.
function Get-FullDescription($Element) {
    if (-not $Element) { return $null }
    try {
        $prop = [System.Windows.Automation.AutomationProperty]::LookupById(30159)
        if ($prop) { return [string]$Element.GetCurrentPropertyValue($prop) }
    } catch { }
    $null
}

function Send-Keys([string] $Keys, [switch] $NoClick) {
    $ok = Activate-Win -NoClick:$NoClick
    if (-not $ok) { Say "  !! window is not foreground; '$Keys' may have gone elsewhere" }
    [System.Windows.Forms.SendKeys]::SendWait($Keys)
    Start-Sleep -Milliseconds 350
}

function Wait-Until([scriptblock] $Condition, [int] $Seconds = 10, [string] $What = 'condition') {
    $deadline = (Get-Date).AddSeconds($Seconds)
    while ((Get-Date) -lt $deadline) {
        try { if (& $Condition) { return $true } } catch { }
        Start-Sleep -Milliseconds 250
    }
    $false
}

# ── state readers ──────────────────────────────────────────────────────────

# The TopBar page title: the topmost Text in the window, i.e. the one with the
# smallest Top that is not inside the 72 px nav rail. Nav items and card
# headers share these strings, so a plain name lookup is ambiguous.
function Get-PageTitle {
    $texts = (Get-Win).FindAll($script:Scope::Descendants,
        (New-Object $script:Cond($script:AE::ControlTypeProperty, $script:CT::Text)))
    $rects = Get-WindowRects
    $best = $null
    foreach ($t in $texts) {
        $r = $t.Current.BoundingRectangle
        if ($r.Width -le 0) { continue }
        if ($r.Left -lt ($rects.Left + 100)) { continue }   # skip the nav rail
        if ($null -eq $best -or $r.Top -lt $best.Current.BoundingRectangle.Top) { $best = $t }
    }
    if ($best) { $best.Current.Name } else { $null }
}

function Get-Enabled($Element) { if ($Element) { $Element.Current.IsEnabled } else { $null } }

# Slint's `accessible-description` reaches UIA as HelpText; the TextField uses
# it for the error line (falling back to the helper line).
function Get-Description($Element) { if ($Element) { $Element.Current.HelpText } else { $null } }

function Get-Checked($Element) {
    $p = $null
    if ($Element.TryGetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern, [ref] $p)) {
        return $p.Current.ToggleState.ToString()
    }
    $null
}

# ── screenshots ────────────────────────────────────────────────────────────

# PrintWindow with PW_RENDERFULLCONTENT into a Bitmap. Asserts the capture is
# not a uniform bitmap: a blank PNG filed as evidence is worse than no PNG.
function Save-WindowShot([string] $File) {
    [void](Activate-Win)
    $r = New-Object NebulaWin32+RECT
    [void][NebulaWin32]::GetWindowRect($script:Hwnd, [ref] $r)
    $w = $r.Right - $r.Left; $h = $r.Bottom - $r.Top
    if ($w -le 0 -or $h -le 0) { throw 'window has no rect (minimised?)' }
    $bmp = New-Object System.Drawing.Bitmap($w, $h)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $hdc = $g.GetHdc()
    $ok = [NebulaWin32]::PrintWindow($script:Hwnd, $hdc, 2)   # PW_RENDERFULLCONTENT
    $g.ReleaseHdc($hdc)
    $g.Dispose()
    if (-not $ok -or (Test-Uniform $bmp)) {
        # Fallback: BitBlt the screen over the window rect.
        $bmp.Dispose()
        $bmp = New-Object System.Drawing.Bitmap($w, $h)
        $g = [System.Drawing.Graphics]::FromImage($bmp)
        $g.CopyFromScreen($r.Left, $r.Top, 0, 0, (New-Object System.Drawing.Size($w, $h)))
        $g.Dispose()
        if (Test-Uniform $bmp) { $bmp.Dispose(); throw "screenshot $File is a uniform bitmap" }
        Say "  (shot $File via CopyFromScreen; PrintWindow returned blank)"
    }
    $path = if ([System.IO.Path]::IsPathRooted($File)) { $File } else { Join-Path $script:ShotDir $File }
    $bmp.Save($path, [System.Drawing.Imaging.ImageFormat]::Png)
    $bmp.Dispose()
    Say "  shot -> $([System.IO.Path]::GetFileName($path))  ($w x $h)"
    $path
}

function Test-Uniform([System.Drawing.Bitmap] $Bmp) {
    $first = $Bmp.GetPixel(0, 0).ToArgb()
    for ($i = 0; $i -lt 200; $i++) {
        $x = Get-Random -Minimum 0 -Maximum $Bmp.Width
        $y = Get-Random -Minimum 0 -Maximum $Bmp.Height
        if ($Bmp.GetPixel($x, $y).ToArgb() -ne $first) { return $false }
    }
    $true
}

# Mean absolute per-pixel difference between two PNGs, over a sampled grid or a
# cropped band. Returns 0..255.
function Compare-Shots([string] $A, [string] $B, [int] $CropTop = -1, [int] $CropHeight = -1, [int] $Step = 6) {
    $ia = [System.Drawing.Bitmap]::FromFile($A)
    $ib = [System.Drawing.Bitmap]::FromFile($B)
    try {
        $y0 = if ($CropTop -ge 0) { $CropTop } else { 0 }
        $y1 = if ($CropHeight -gt 0) { [Math]::Min($ia.Height, $y0 + $CropHeight) } else { $ia.Height }
        $y1 = [Math]::Min($y1, $ib.Height)
        $diff = 0.0; $n = 0
        for ($y = $y0; $y -lt $y1; $y += $Step) {
            for ($x = 0; $x -lt [Math]::Min($ia.Width, $ib.Width); $x += $Step) {
                $pa = $ia.GetPixel($x, $y); $pb = $ib.GetPixel($x, $y)
                $diff += [Math]::Abs($pa.R - $pb.R) + [Math]::Abs($pa.G - $pb.G) + [Math]::Abs($pa.B - $pb.B)
                $n += 3
            }
        }
        if ($n -eq 0) { return 0 }
        [Math]::Round($diff / $n, 3)
    } finally { $ia.Dispose(); $ib.Dispose() }
}

# Mean absolute per-pixel difference over one window-relative rectangle, for
# "did a focus ring appear on *this* control".
function Compare-Region([string] $A, [string] $B, [int] $Left, [int] $Top, [int] $W, [int] $H) {
    $ia = [System.Drawing.Bitmap]::FromFile($A)
    $ib = [System.Drawing.Bitmap]::FromFile($B)
    try {
        $x1 = [Math]::Min([Math]::Min($ia.Width, $ib.Width), $Left + $W)
        $y1 = [Math]::Min([Math]::Min($ia.Height, $ib.Height), $Top + $H)
        $x0 = [Math]::Max(0, $Left); $y0 = [Math]::Max(0, $Top)
        $diff = 0.0; $n = 0
        for ($y = $y0; $y -lt $y1; $y++) {
            for ($x = $x0; $x -lt $x1; $x++) {
                $pa = $ia.GetPixel($x, $y); $pb = $ib.GetPixel($x, $y)
                $diff += [Math]::Abs($pa.R - $pb.R) + [Math]::Abs($pa.G - $pb.G) + [Math]::Abs($pa.B - $pb.B)
                $n += 3
            }
        }
        if ($n -eq 0) { return 0 }
        [Math]::Round($diff / $n, 3)
    } finally { $ia.Dispose(); $ib.Dispose() }
}

# The window-relative rect of an element, for Compare-Region.
function Get-Rect($Element) {
    $r = $Element.Current.BoundingRectangle
    $w = Get-WindowRects
    [pscustomobject]@{
        Left = [int]($r.Left - $w.Left); Top = [int]($r.Top - $w.Top)
        W = [int]$r.Width; H = [int]$r.Height
    }
}

function Get-Focused {
    try { [System.Windows.Automation.AutomationElement]::FocusedElement } catch { $null }
}

# ── tree dump ──────────────────────────────────────────────────────────────

function Dump-Tree([string] $File) {
    $walker = [System.Windows.Automation.TreeWalker]::ControlViewWalker
    $lines = New-Object System.Collections.Generic.List[string]
    function Walk($e, [int] $depth) {
        if ($depth -gt 40) { return }
        $c = $e.Current
        $r = $c.BoundingRectangle
        $lines.Add(("{0}{1} | Name='{2}' | AutomationId='{3}' | Enabled={4} | Off={5} | Rect=({6},{7},{8}x{9})" -f `
            ('  ' * $depth), $c.ControlType.ProgrammaticName.Replace('ControlType.', ''), $c.Name, $c.AutomationId,
            $c.IsEnabled, $c.IsOffscreen, [int]$r.Left, [int]$r.Top, [int]$r.Width, [int]$r.Height))
        $child = $walker.GetFirstChild($e)
        while ($child -ne $null) {
            Walk $child ($depth + 1)
            $child = $walker.GetNextSibling($child)
        }
    }
    Walk (Get-Win) 0
    $path = if ([System.IO.Path]::IsPathRooted($File)) { $File } else { Join-Path $script:ShotDir $File }
    Set-Content -LiteralPath $path -Value $lines -Encoding UTF8
    Say "  tree -> $([System.IO.Path]::GetFileName($path))  ($($lines.Count) nodes)"
    $lines.Count
}

# ── explorer windows (폴더 열기) ────────────────────────────────────────────

function Get-ExplorerWindows {
    $found = New-Object System.Collections.Generic.List[string]
    $cb = [NebulaWin32+EnumProc] {
        param($h, $p)
        if ([NebulaWin32]::IsWindowVisible($h)) {
            $sb = New-Object System.Text.StringBuilder 260
            [void][NebulaWin32]::GetClassName($h, $sb, 260)
            if ($sb.ToString() -eq 'CabinetWClass') {
                $tb = New-Object System.Text.StringBuilder 512
                [void][NebulaWin32]::GetWindowTextW($h, $tb, 512)
                $found.Add("$($h.ToInt64()):$($tb.ToString())")
            }
        }
        $true
    }
    [void][NebulaWin32]::EnumWindows($cb, [IntPtr]::Zero)
    $found
}

function Close-ExplorerWindow([long] $Handle) {
    [void][NebulaWin32]::PostMessage([IntPtr]$Handle, [NebulaWin32]::WM_CLOSE, [IntPtr]::Zero, [IntPtr]::Zero)
}

function Close-AppWindow {
    [void][NebulaWin32]::PostMessage($script:Hwnd, [NebulaWin32]::WM_CLOSE, [IntPtr]::Zero, [IntPtr]::Zero)
}

function End-Log {
    if ($script:Failures.Count -eq 0) { Say 'RESULT: all checks passed' }
    else { Say ("RESULT: {0} failed check(s): {1}" -f $script:Failures.Count, ($script:Failures -join ' | ')) }
}
