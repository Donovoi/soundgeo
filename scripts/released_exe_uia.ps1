param([string]$Action='snapshot',[string]$OutputPath,[string]$MediaPath)
$ErrorActionPreference='Stop'
if ($env:GITHUB_ACTIONS -ne 'true') { throw 'Windows VM only' }
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
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
if ($Action -eq 'set_media') {
    $first=$rows | Where-Object {$_.type -eq 'ControlType.Edit' -and $_.editable -and $_.enabled -and -not $_.offscreen} | Sort-Object {$_.box[1]} | Select-Object -First 1
    if ($null -eq $first) { throw 'No visible editable recording field found' }
    $node=$nodes.Item($first.index)
    $node.SetFocus()
    $pattern=$node.GetCurrentPattern([Windows.Automation.ValuePattern]::Pattern)
    $pattern.SetValue($MediaPath)
}
