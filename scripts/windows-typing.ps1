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
    // As large as the union's largest member (MOUSEINPUT, 32 bytes on x64): SendInput refuses
    // any other INPUT size with error 87.
    [StructLayout(LayoutKind.Explicit)]
    struct INPUTUNION { [FieldOffset(0)] public KEYBDINPUT ki; [FieldOffset(0)] public long pad0; [FieldOffset(8)] public long pad1; [FieldOffset(16)] public long pad2; [FieldOffset(24)] public long pad3; }
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

# The first text field whose name matches. A label shows the same name ("סיסמה" above the
# field) and cannot take the keyboard, so only an edit control that can is taken.
function Find-Field($root, [string] $pattern, [string] $what, [int] $seconds = 60) {
    $edit = [System.Windows.Automation.ControlType]::Edit
    $deadline = (Get-Date).AddSeconds($seconds)
    while ((Get-Date) -lt $deadline) {
        $all = $root.FindAll($Tree, [System.Windows.Automation.Condition]::TrueCondition)
        foreach ($e in $all) {
            $c = $e.Current
            if ($c.ControlType -eq $edit -and $c.IsKeyboardFocusable -and $c.IsEnabled -and $c.Name -match $pattern) { return $e }
        }
        Start-Sleep -Milliseconds 300
    }
    Show-Names $root
    throw "not found: $what ($pattern)"
}

# The first element whose name matches and that can be pressed. Text inside a button (or a
# heading with the same words) shows the same name and cannot, so only one that can is taken.
function Invoke-Named($root, [string] $pattern, [string] $what, [int] $seconds = 60) {
    $invoke = [System.Windows.Automation.InvokePattern]::Pattern
    $deadline = (Get-Date).AddSeconds($seconds)
    while ((Get-Date) -lt $deadline) {
        $all = $root.FindAll($Tree, [System.Windows.Automation.Condition]::TrueCondition)
        foreach ($e in $all) {
            if ($e.Current.Name -notmatch $pattern) { continue }
            $p = $null
            if ($e.TryGetCurrentPattern($invoke, [ref] $p)) {
                $p.Invoke()
                Write-Host "pressed: $($e.Current.Name)"
                return
            }
        }
        Start-Sleep -Milliseconds 300
    }
    Show-Names $root
    throw "nothing to press: $what ($pattern)"
}

# A menu and its items: a menu button opens on a key or a pointer press, not on the click
# Invoke sends, and offers no Invoke at all; an item invoked while its menu settles may be
# gone before it acts. So with -Keyboard (and for anything that cannot be invoked) the
# element takes the focus and Enter, as she would choose it from the keyboard.
function Press-Named($root, [string] $pattern, [string] $what, [int] $seconds = 60, [switch] $Keyboard) {
    $invoke = [System.Windows.Automation.InvokePattern]::Pattern
    $deadline = (Get-Date).AddSeconds($seconds)
    while ((Get-Date) -lt $deadline) {
        $all = $root.FindAll($Tree, [System.Windows.Automation.Condition]::TrueCondition)
        foreach ($e in $all) {
            $c = $e.Current
            if ($c.Name -notmatch $pattern -or -not $c.IsEnabled) { continue }
            $p = $null
            $keys = $c.IsKeyboardFocusable -and ($Keyboard -or -not $e.TryGetCurrentPattern($invoke, [ref] $p))
            if (-not $keys -and $e.TryGetCurrentPattern($invoke, [ref] $p)) {
                $p.Invoke()
                Write-Host "pressed: $($c.Name)"
                return
            }
            if ($keys) {
                Focus-Field $e $what
                [Keys]::Enter()
                Write-Host "pressed with Enter: $($c.Name)"
                return
            }
        }
        Start-Sleep -Milliseconds 300
    }
    Show-Names $root
    throw "nothing to press: $what ($pattern)"
}

# Where the keys go now, for the log when they went elsewhere.
function Focused-Name {
    $f = [System.Windows.Automation.AutomationElement]::FocusedElement
    if ($null -eq $f) { return "(nothing)" }
    return "$($f.Current.ControlType.ProgrammaticName) '$($f.Current.Name)'"
}

# Focus the field, and keep at it until it holds the focus: a dialog that opens moves the
# focus to its first field by itself, a moment after it shows.
function Focus-Field($element, [string] $what) {
    Start-Sleep -Milliseconds 1000
    for ($i = 0; $i -lt 20; $i++) {
        $element.SetFocus()
        Start-Sleep -Milliseconds 250
        if ($element.Current.HasKeyboardFocus) {
            Start-Sleep -Milliseconds 250
            if ($element.Current.HasKeyboardFocus) { return }
        }
    }
    throw "the focus does not stay in $what; it is in $(Focused-Name)"
}

function Type-Into($element, [string] $text, [int] $pauseMs) {
    Focus-Field $element "the field"
    foreach ($c in $text.ToCharArray()) {
        [Keys]::Char($c)
        if ($pauseMs -gt 0) { Start-Sleep -Milliseconds $pauseMs }
    }
}

# The text in a field: its value, or for a long text box that offers only the text pattern,
# the whole of its text.
function Value-Of($element) {
    $p = $null
    if ($element.TryGetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern, [ref] $p)) {
        return $p.Current.Value
    }
    if ($element.TryGetCurrentPattern([System.Windows.Automation.TextPattern]::Pattern, [ref] $p)) {
        return $p.DocumentRange.GetText(-1)
    }
    throw "the text field offers neither its value nor its text"
}

$window = Get-Window
[Keys]::SetForegroundWindow([IntPtr]$window.Current.NativeWindowHandle) | Out-Null

# 1. Open the vault.
# Not `$password`: PowerShell names ignore case, and that is the [string] parameter above.
$passwordField = Find-Field $window '^סיסמה$' "the password field" 90
Type-Into $passwordField $Password 0
[Keys]::Enter()

# 2. The case, then its materials, then "הדבקת טקסט" for one of them (since round 49 it is in
#    the menu of each kind of material, here the earlier assessments).
Invoke-Named $window 'בתיק של' "a case on the cases screen"
Invoke-Named $window 'חומרים$' "the materials stage"
Press-Named $window '^הוספה לאבחונים קודמים' "the add menu of the earlier assessments" -Keyboard
Press-Named $window '^הדבקת טקסט' "paste text for a material" -Keyboard
$field = Find-Field $window '^הטקסט$' "the text field of the material"

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
    Write-Host "the focus is in: $(Focused-Name)"
    throw "typing lost or changed keys: $($got.Length) of $($text.Length) characters, first difference at $at"
}
Write-Host ("typing: ok ({0} Hebrew characters in {1:N1} s, none lost)" -f $text.Length, $typed)
