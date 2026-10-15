# Release gate (F1 (d)): typing in the real window, on WebView2, as she types. The installed
# app is open on a kept vault (invented data, tests/fixtures/vaults); this opens it with the
# fixtures' password, opens the case, opens "הדבקת טקסט" for a material, and types 500 Hebrew
# characters one key at a time through Windows' own input (SendInput, Unicode), while the
# window's status checks run. The text field must then hold exactly what was typed: no key
# lost, doubled or out of order, and the window must have kept up.
#
# Runs in Windows PowerShell 5.1 (UI Automation ships with it). Reads the window only through
# UI Automation, the way a screen reader does; nothing in the app is switched on for the test.
param(
    [Parameter(Mandatory = $true)] [int] $ProcessId,
    [string] $Password = "כלב ירוק רץ מהר בגינה",
    [int] $Characters = 500
)
$ErrorActionPreference = "Stop"
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
public static class Keys {
    [StructLayout(LayoutKind.Sequential)]
    struct KEYBDINPUT { public ushort wVk; public ushort wScan; public uint dwFlags; public uint time; public IntPtr dwExtraInfo; }
    [StructLayout(LayoutKind.Explicit)]
    struct INPUTUNION { [FieldOffset(0)] public KEYBDINPUT ki; [FieldOffset(0)] public long pad0; [FieldOffset(8)] public long pad1; [FieldOffset(16)] public long pad2; }
    [StructLayout(LayoutKind.Sequential)]
    struct INPUT { public uint type; public INPUTUNION u; }
    [DllImport("user32.dll", SetLastError = true)]
    static extern uint SendInput(uint n, INPUT[] inputs, int size);
    [DllImport("user32.dll")]
    public static extern bool SetForegroundWindow(IntPtr hwnd);
    const uint KEYBOARD = 1, KEYUP = 0x2, UNICODE = 0x4;
    static void Send(ushort vk, ushort scan, uint flags) {
        var i = new INPUT[1];
        i[0].type = KEYBOARD;
        i[0].u.ki = new KEYBDINPUT { wVk = vk, wScan = scan, dwFlags = flags };
        if (SendInput(1, i, Marshal.SizeOf(typeof(INPUT))) != 1) throw new Exception("SendInput failed: " + Marshal.GetLastWin32Error());
    }
    public static void Char(char c) { Send(0, c, UNICODE); Send(0, c, UNICODE | KEYUP); }
    public static void Enter() { Send(0x0D, 0, 0); Send(0x0D, 0, KEYUP); }
}
"@

$A = [System.Windows.Automation.AutomationElement]
$Tree = [System.Windows.Automation.TreeScope]::Descendants

function Get-Window {
    for ($i = 0; $i -lt 120; $i++) {
        $p = Get-Process -Id $ProcessId -ErrorAction Stop
        if ($p.MainWindowHandle -ne [IntPtr]::Zero) { return $A::FromHandle($p.MainWindowHandle) }
        Start-Sleep -Milliseconds 500
    }
    throw "the app shows no window"
}

function Show-Names($root) {
    $all = $root.FindAll($Tree, [System.Windows.Automation.Condition]::TrueCondition)
    $names = foreach ($e in $all) { if ($e.Current.Name) { "$($e.Current.ControlType.ProgrammaticName): $($e.Current.Name)" } }
    Write-Host "--- what the window shows ---"
    $names | Select-Object -First 200 | ForEach-Object { Write-Host $_ }
}

# The first element whose name matches, waiting for the window to show it.
function Find-Named($root, [string] $pattern, [string] $what, [int] $seconds = 60) {
    $deadline = (Get-Date).AddSeconds($seconds)
    while ((Get-Date) -lt $deadline) {
        $all = $root.FindAll($Tree, [System.Windows.Automation.Condition]::TrueCondition)
        foreach ($e in $all) {
            if ($e.Current.Name -match $pattern) { return $e }
        }
        Start-Sleep -Milliseconds 300
    }
    Show-Names $root
    throw "not found: $what ($pattern)"
}

function Invoke-Named($root, [string] $pattern, [string] $what) {
    $e = Find-Named $root $pattern $what
    $e.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
    Write-Host "pressed: $($e.Current.Name)"
}

function Type-Into($element, [string] $text, [int] $pauseMs) {
    $element.SetFocus()
    Start-Sleep -Milliseconds 300
    foreach ($c in $text.ToCharArray()) {
        [Keys]::Char($c)
        if ($pauseMs -gt 0) { Start-Sleep -Milliseconds $pauseMs }
    }
}

function Value-Of($element) {
    $element.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).Current.Value
}

$window = Get-Window
[Keys]::SetForegroundWindow([IntPtr]$window.Current.NativeWindowHandle) | Out-Null

# 1. Open the vault.
# Not `$password`: PowerShell names ignore case, and that is the [string] parameter above.
$passwordField = Find-Named $window '^סיסמה$' "the password field" 90
Type-Into $passwordField $Password 0
[Keys]::Enter()

# 2. The case, then its materials, then "הדבקת טקסט" for one of them.
Invoke-Named $window 'בתיק של' "a case on the cases screen"
Invoke-Named $window 'חומרים$' "the materials stage"
Invoke-Named $window '^הדבקת טקסט' "paste text for a material"
$field = Find-Named $window '^הטקסט$' "the text field of the material"

# 3. 500 Hebrew characters, one key at a time, words and punctuation as she writes them.
$words = @("הילד", "שיחק", "בחצר", "עם", "חברים,", "והתמיד", "במשימה", "גם", "כשהיה", "לו", "קשה.")
$sb = New-Object System.Text.StringBuilder
for ($i = 0; $sb.Length -lt $Characters; $i++) {
    if ($sb.Length -gt 0) { [void]$sb.Append(" ") }
    [void]$sb.Append($words[$i % $words.Count])
}
$text = $sb.ToString().Substring(0, $Characters)
$started = Get-Date
Type-Into $field $text 15
$typed = ((Get-Date) - $started).TotalSeconds

# 4. Exactly what was typed, once the window caught up.
$got = ""
for ($i = 0; $i -lt 20; $i++) {
    $got = Value-Of $field
    if ($got -eq $text) { break }
    Start-Sleep -Milliseconds 250
}
if ($got -ne $text) {
    $n = [Math]::Min($got.Length, $text.Length)
    $at = 0
    while ($at -lt $n -and $got[$at] -eq $text[$at]) { $at++ }
    throw "typing lost or changed keys: $($got.Length) of $($text.Length) characters, first difference at $at"
}
Write-Host ("typing: ok ({0} Hebrew characters in {1:N1} s, none lost)" -f $text.Length, $typed)
