param([string]$Action='snapshot',[string]$OutputPath,[string]$MediaPath,[string]$MatchName,[string]$MatchHelp,[string]$MatchValue,[string]$MatchType,[int]$Ordinal=0,[string]$NewValue)
$ErrorActionPreference='Stop'
if ($env:GITHUB_ACTIONS -ne 'true') { throw 'Windows VM only' }
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
Add-Type -AssemblyName System.Windows.Forms
Add-Type -TypeDefinition 'using System; using System.Runtime.InteropServices; public class FieldMouse { [DllImport("user32.dll")] public static extern bool SetCursorPos(int x,int y); [DllImport("user32.dll")] public static extern void mouse_event(uint f,uint x,uint y,int data,UIntPtr extra); }'
$condition=New-Object Windows.Automation.PropertyCondition([Windows.Automation.AutomationElement]::NameProperty,'SoundGeo')
$window=[Windows.Automation.AutomationElement]::RootElement.FindFirst([Windows.Automation.TreeScope]::Children,$condition)
if ($null -eq $window) { throw 'SoundGeo window not found' }
$nodes=$window.FindAll([Windows.Automation.TreeScope]::Descendants,[Windows.Automation.Condition]::TrueCondition)
$rows=@()
for ($i=0;$i -lt $nodes.Count;$i++) {
    $node=$nodes.Item($i)
    $current=$node.Current
    $value=$null
    $pattern=$null
    $editable=$false
    if ($node.TryGetCurrentPattern([Windows.Automation.ValuePattern]::Pattern,[ref]$pattern)) {
        $value=$pattern.Current.Value
        $editable=-not $pattern.Current.IsReadOnly
    }
    $box=$current.BoundingRectangle
    $rows += [pscustomobject]@{index=$i;name=$current.Name;type=$current.ControlType.ProgrammaticName;id=$current.AutomationId;value=$value;editable=$editable;enabled=$current.IsEnabled;offscreen=$current.IsOffscreen;help=$current.HelpText;box=@($box.X,$box.Y,$box.Width,$box.Height)}
}
$rows | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath $OutputPath -Encoding UTF8
if ($Action -eq 'escape') { [Windows.Forms.SendKeys]::SendWait('{ESC}'); exit }
if ($Action -in @('invoke','set','patterns','reveal')) {
    $matched=@($rows | Where-Object { (!$MatchName -or $_.name -eq $MatchName) -and (!$MatchHelp -or $_.help -eq $MatchHelp) -and (!$MatchValue -or $_.value -eq $MatchValue) -and (!$MatchType -or $_.type -eq $MatchType) })
    if ($matched.Count -le $Ordinal) { throw "No matching control: $MatchName $MatchHelp $MatchValue $MatchType [$Ordinal]" }
    $selected=$matched[$Ordinal]
    $node=$nodes.Item($selected.index)
    if ($Action -eq 'patterns') { $node.GetSupportedPatterns() | ForEach-Object {$_.ProgrammaticName}; exit }
    if ($selected.box[1] -lt 90 -or $selected.box[1]+$selected.box[3] -gt 690) {
        if ($Action -eq 'reveal') {
            [FieldMouse]::SetCursorPos(170,580) | Out-Null
            $delta=if ($selected.box[1] -lt 90) {480} else {-480}
            [FieldMouse]::mouse_event(0x800,0,0,$delta,[UIntPtr]::Zero)
            exit
        }
        throw 'Selected control is outside the application viewport'
    }
    if ($Action -in @('invoke','set')) {
        if (-not $selected.enabled) { throw 'Selected control is disabled' }
        [FieldMouse]::SetCursorPos([int]($selected.box[0]+$selected.box[2]/2),[int]($selected.box[1]+$selected.box[3]/2)) | Out-Null
        [FieldMouse]::mouse_event(2,0,0,0,[UIntPtr]::Zero)
        [FieldMouse]::mouse_event(4,0,0,0,[UIntPtr]::Zero)
        if ($Action -eq 'set') {
            if ($NewValue -notmatch '^[A-Za-z0-9_:/\\ .-]+$') { throw 'Unsupported test value for literal keyboard entry' }
            # A second click activates egui DragValue's text editor.
            Start-Sleep -Milliseconds 80
            [FieldMouse]::mouse_event(2,0,0,0,[UIntPtr]::Zero)
            [FieldMouse]::mouse_event(4,0,0,0,[UIntPtr]::Zero)
            Start-Sleep -Milliseconds 100
            [Windows.Forms.SendKeys]::SendWait('^a')
            [Windows.Forms.SendKeys]::SendWait($NewValue)
            [Windows.Forms.SendKeys]::SendWait('{TAB}')
        }
    }
}
if ($Action -eq 'set_media') {
    $first=$rows | Where-Object {$_.type -eq 'ControlType.Edit' -and $_.editable -and $_.enabled -and -not $_.offscreen} | Sort-Object {$_.box[1]} | Select-Object -First 1
    if ($null -eq $first) { throw 'No visible editable recording field found' }
    $node=$nodes.Item($first.index)
    $node.SetFocus()
    $pattern=$node.GetCurrentPattern([Windows.Automation.ValuePattern]::Pattern)
    $pattern.SetValue($MediaPath)
}
