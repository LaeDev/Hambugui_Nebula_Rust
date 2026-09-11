# drive-mods.ps1 — additions on top of the worktree's drive.ps1 for the 모드 lane.
# Dot-source AFTER drive.ps1. Must stay UTF-8 with BOM (Korean literals).

. 'H:\nebularust\docs\verification\gui-qa\drive.ps1'

# The 모드 page is fully `accessible-id`'d, so match on AutomationId, not Name.
function Find-ById([string] $Id, [int] $TimeoutSeconds = 6, [switch] $Optional) {
    $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    do {
        $all = @((Get-Win).FindAll($script:Scope::Descendants,
            (New-Object $script:Cond($script:AE::AutomationIdProperty, $Id))))
        if ($all.Count) { return $all[0] }
        Start-Sleep -Milliseconds 150
    } while ((Get-Date) -lt $deadline)
    if ($Optional) { return $null }
    throw "no element with AutomationId '$Id'"
}

function Find-ByIdPrefix([string] $Prefix) {
    @((Get-Win).FindAll($script:Scope::Descendants,
        [System.Windows.Automation.Condition]::TrueCondition)) |
        Where-Object { $_.Current.AutomationId -and $_.Current.AutomationId.StartsWith($Prefix) }
}

function Get-Ids {
    @((Get-Win).FindAll($script:Scope::Descendants,
        [System.Windows.Automation.Condition]::TrueCondition)) |
        Where-Object { $_.Current.AutomationId } |
        ForEach-Object { "{0}|{1}|{2}" -f $_.Current.AutomationId,
            $_.Current.ControlType.ProgrammaticName.Replace('ControlType.',''), $_.Current.Name }
}

# UIA LiveSetting (30135): 0 Off, 1 Polite, 2 Assertive.
function Get-LiveSetting($Element) {
    if (-not $Element) { return $null }
    try {
        $p = [System.Windows.Automation.AutomationProperty]::LookupById(30135)
        if ($p) { return [string]$Element.GetCurrentPropertyValue($p) }
    } catch { }
    $null
}

# Every visible Text node's Name, for "does this string appear on screen".
function Get-Texts {
    @((Get-Win).FindAll($script:Scope::Descendants,
        (New-Object $script:Cond($script:AE::ControlTypeProperty, $script:CT::Text)))) |
        ForEach-Object { $_.Current.Name } | Where-Object { $_ }
}

function Has-Text([string] $Fragment) {
    [bool](@(Get-Texts) | Where-Object { $_.Contains($Fragment) })
}

function Find-TextLike([string] $Fragment) {
    @(Get-Texts) | Where-Object { $_.Contains($Fragment) }
}

# Any element (any role) whose Name contains the fragment.
function Find-AnyByPartial([string] $Fragment, [int] $TimeoutSeconds = 6) {
    $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    do {
        foreach ($e in (Get-Win).FindAll($script:Scope::Descendants,
                [System.Windows.Automation.Condition]::TrueCondition)) {
            if ($e.Current.Name -and $e.Current.Name.Contains($Fragment)) { return $e }
        }
        Start-Sleep -Milliseconds 150
    } while ((Get-Date) -lt $deadline)
    $null
}

function Focused-Id {
    $f = Get-Focused
    if ($f) { try { $f.Current.AutomationId } catch { '' } } else { '' }
}

function Focused-Desc {
    $f = Get-Focused
    if (-not $f) { return '<none>' }
    try {
        $c = $f.Current
        "{0} / {1} / {2}" -f $c.ControlType.ProgrammaticName.Replace('ControlType.',''), $c.Name, $c.AutomationId
    } catch { '<stale>' }
}

function Get-Patterns($Element) {
    if (-not $Element) { return '<null>' }
    (@($Element.GetSupportedPatterns()) | ForEach-Object { $_.ProgrammaticName }) -join ','
}

# SelectionItemPattern.IsSelected, the UIA carrier a radio button is read by.
function Get-Selected($Element) {
    if (-not $Element) { return $null }
    $p = $null
    if ($Element.TryGetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern, [ref] $p)) {
        return [string]$p.Current.IsSelected
    }
    $null
}

function Wait-NotBusy([int] $Seconds = 180) {
    Wait-Until { -not (Find-AnyByPartial '모드 설치 중' 1) } $Seconds 'install pill gone'
}
